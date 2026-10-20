use super::support::*;
use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::{ArtifactDigest, Run, orders::*};

struct Model {
    calls: usize,
    identity: (SemanticReference, ArtifactDigest),
    output: Result<Option<ProposedFill>, OrderError>,
}
impl SimulationExecution for Model {
    fn identity(&self) -> (SemanticReference, ArtifactDigest) {
        self.identity
    }
    fn propose(
        &mut self,
        request: SimulationRequest<'_, '_>,
    ) -> Result<Option<ProposedFill>, OrderError> {
        self.calls += 1;
        assert_eq!(request.at(), request.evidence().assessment().0);
        assert_eq!(
            request.remaining().parts().0,
            request.order().terms().2.parts().0
        );
        self.output
    }
}
fn step<'a, 'tracker>(
    id: OrderEventId,
    revision: u64,
    ready: &'a ReadyEvidence<'tracker>,
    artifacts: &'a [ArtifactDigest],
) -> TestResult<SimulationStep<'a, 'tracker>> {
    Ok(SimulationStep::new(
        id,
        revision,
        ready,
        model_identity()?,
        digest(),
        artifacts,
    )?)
}
fn portable<T: Send + Sync>() {}

#[test]
fn later_readiness_reuses_frozen_evidence_without_rewriting_availability() -> TestResult {
    let evidence = Evidence::trade()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    run.next_input();
    let mut order = Order::new(definition(&mut run, OrderSide::BuyToOpen, 10)?, 8)?;
    apply(&mut run, &mut order, 1, OrderEventKind::Accepted)?;
    run.advance_to(at(2)?)?;
    let ready = evidence.ready_at(2)?;
    let mut model = Model {
        calls: 0,
        identity: model_identity()?,
        output: Ok(None),
    };
    let id = OrderEventId::new(run.next_id()?);
    let fact = order.simulate(&run, &mut model, step(id, 2, &ready, &[digest()])?)?;
    assert_eq!(fact.effect().0, at(2)?);
    assert_eq!(run.consumed_inputs()[0].available_at(), at(1)?);
    assert_eq!(model.calls, 1);
    // Old assessment is not authority at a later execution time; the actual
    // P04 readiness policy must qualify the evidence at the new instant.
    let old = evidence.ready()?;
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 2, &old, &[digest()])?),
        Err(OrderError::Evidence)
    );
    assert!(evidence.ready_at(30).is_err());
    Ok(())
}

#[test]
fn proposal_is_uncommitted_and_retained_fill_is_not_resampled_after_restart() -> TestResult {
    let evidence = Evidence::trade()?;
    let ready = evidence.ready()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    assert!(run.next_input().is_some());
    let definition = definition(&mut run, OrderSide::BuyToOpen, 10)?;
    let mut order = Order::new(definition.clone(), 16)?;
    apply(&mut run, &mut order, 1, OrderEventKind::Accepted)?;
    let checkpoint = run.checkpoint();
    let id = OrderEventId::new(run.next_id()?);
    let artifacts = [digest()];
    let mut model = Model {
        calls: 0,
        identity: model_identity()?,
        output: Ok(Some(ProposedFill::new(quantity(3)?, price(99)?))),
    };
    let fact = order.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?)?;
    assert_eq!(order.state(), (OrderStatus::Working, 1));
    assert_eq!(order.quantities().0, None);
    order.apply(fact)?;
    assert_eq!(model.calls, 1);
    // Reconstruct authoritative history and restore the run's pre-attempt continuation.
    let mut replay = Order::new(definition, 16)?;
    for event in order.events() {
        replay.apply(*event)?;
    }
    let mut restored = Run::restore(&manifest, &checkpoint)?;
    assert_eq!(OrderEventId::new(restored.next_id()?), id);
    model.output = Err(OrderError::ModelUnavailable);
    assert_eq!(
        replay.simulate(&restored, &mut model, step(id, 2, &ready, &artifacts)?)?,
        fact
    );
    assert_eq!(replay.apply(fact)?, ApplyOutcome::Duplicate);
    assert_eq!(model.calls, 1);
    assert_eq!(replay.quantities(), order.quantities());
    portable::<Order>();
    portable::<OrderDefinition>();
    portable::<OrderEvent>();
    Ok(())
}

#[test]
fn no_fill_is_a_retained_decision_and_conflicting_attempts_fail_closed() -> TestResult {
    let evidence = Evidence::trade()?;
    let ready = evidence.ready()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    run.next_input();
    let mut order = Order::new(definition(&mut run, OrderSide::BuyToOpen, 10)?, 2)?;
    apply(&mut run, &mut order, 1, OrderEventKind::Accepted)?;
    let id = OrderEventId::new(run.next_id()?);
    let artifacts = [digest()];
    let mut model = Model {
        calls: 0,
        identity: model_identity()?,
        output: Ok(None),
    };
    let fact = order.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?)?;
    assert!(matches!(fact.effect().1, OrderEventKind::NoFill { .. }));
    order.apply(fact)?;
    assert_eq!(order.state(), (OrderStatus::Working, 2));
    model.output = Ok(Some(ProposedFill::new(quantity(10)?, price(100)?)));
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?)?,
        fact
    );
    assert_eq!(model.calls, 1);
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 3, &ready, &artifacts)?),
        Err(OrderError::IdentityConflict)
    );
    let changed = SimulationStep::new(
        id,
        2,
        &ready,
        model_identity()?,
        ArtifactDigest::of(b"changed-evidence"),
        &artifacts,
    )?;
    assert_eq!(
        order.simulate(&run, &mut model, changed),
        Err(OrderError::IdentityConflict)
    );
    let new_id = OrderEventId::new(run.next_id()?);
    assert_eq!(
        order.simulate(&run, &mut model, step(new_id, 3, &ready, &artifacts)?),
        Err(OrderError::Capacity)
    );
    assert_eq!(model.calls, 1);
    Ok(())
}

#[test]
fn run_time_frozen_artifacts_and_model_are_checked_before_execution() -> TestResult {
    let evidence = Evidence::trade()?;
    let ready = evidence.ready()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let mut order = Order::new(definition(&mut run, OrderSide::BuyToOpen, 10)?, 8)?;
    apply(&mut run, &mut order, 1, OrderEventKind::Accepted)?;
    let id = OrderEventId::new(run.next_id()?);
    let artifacts = [digest()];
    let mut model = Model {
        calls: 0,
        identity: model_identity()?,
        output: Ok(None),
    };
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?),
        Err(OrderError::Evidence)
    );
    run.advance_to(at(1)?)?; // Matching time, but the frozen input is still unconsumed.
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?),
        Err(OrderError::Evidence)
    );
    run.next_input();
    for artifacts in [&[][..], &[ArtifactDigest::of(b"different")][..]] {
        assert_eq!(
            order.simulate(&run, &mut model, step(id, 2, &ready, artifacts)?),
            Err(OrderError::Evidence)
        );
    }
    model.identity.1 = ArtifactDigest::of(b"wrong-model-config");
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?),
        Err(OrderError::ModelMismatch)
    );
    model.identity = model_identity()?;
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 3, &ready, &artifacts)?),
        Err(OrderError::RevisionGap)
    );
    run.advance_to(at(2)?)?;
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?),
        Err(OrderError::Evidence)
    );
    assert_eq!(model.calls, 0);
    Ok(())
}

#[test]
fn invalid_model_outputs_and_failures_never_change_the_projection() -> TestResult {
    let evidence = Evidence::trade()?;
    let ready = evidence.ready()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    run.next_input();
    let mut order = Order::new(definition(&mut run, OrderSide::BuyToOpen, 10)?, 8)?;
    apply(&mut run, &mut order, 1, OrderEventKind::Accepted)?;
    let id = OrderEventId::new(run.next_id()?);
    let artifacts = [digest()];
    let mut model = Model {
        calls: 0,
        identity: model_identity()?,
        output: Ok(None),
    };
    for (output, error) in [
        (
            Err(OrderError::ModelUnavailable),
            OrderError::ModelUnavailable,
        ),
        (
            Ok(Some(ProposedFill::new(quantity(11)?, price(100)?))),
            OrderError::Quantity,
        ),
        (
            Ok(Some(ProposedFill::new(quantity(1)?, price(101)?))),
            OrderError::Price,
        ),
        (
            Ok(Some(ProposedFill::new(
                quantity(1)?,
                OrderPrice::Premium(OptionPoints::new(1, 0)?),
            ))),
            OrderError::Unit,
        ),
    ] {
        model.output = output;
        assert_eq!(
            order.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?),
            Err(error)
        );
        assert_eq!(order.state(), (OrderStatus::Working, 1));
        assert_eq!(order.events().len(), 1);
    }
    apply(&mut run, &mut order, 1, fill(10, 100)?)?;
    let calls = model.calls;
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 3, &ready, &artifacts)?),
        Err(OrderError::State)
    );
    assert_eq!(model.calls, calls);
    Ok(())
}

#[test]
fn a_frozen_index_level_never_substitutes_for_equity_trade_evidence() -> TestResult {
    let evidence = Evidence::new(
        1,
        ObservationValue::IndexLevel(IndexPoints::new(100, 0)?),
        EvidenceKind::IndexLevel,
        MarketScope::IndexPublisher,
    )?;
    let ready = evidence.ready()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    run.next_input();
    let mut order = Order::new(definition(&mut run, OrderSide::BuyToOpen, 10)?, 8)?;
    apply(&mut run, &mut order, 1, OrderEventKind::Accepted)?;
    let mut model = Model {
        calls: 0,
        identity: model_identity()?,
        output: Ok(None),
    };
    let id = OrderEventId::new(run.next_id()?);
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 2, &ready, &[digest()])?),
        Err(OrderError::Evidence)
    );
    assert_eq!(model.calls, 0);
    Ok(())
}

#[test]
fn option_quotes_require_exact_specification_and_enforce_fill_ticks() -> TestResult {
    let specification = option(ExerciseStyle::European, SettlementStyle::Cash)?;
    let quote = OptionQuoteSnapshot::new(
        &specification,
        QuoteSide::new(OptionPoints::new(100, 2)?, None, at(0)?),
        QuoteSide::new(OptionPoints::new(105, 2)?, None, at(0)?),
    )?;
    let evidence = Evidence::new(
        10,
        ObservationValue::OptionQuote(quote),
        EvidenceKind::OptionSnapshot,
        MarketScope::Consolidated,
    )?;
    let ready = evidence.ready()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    run.next_input();
    let scope = definition(&mut run, OrderSide::BuyToOpen, 1)?.scope();
    let terms = OrderDefinition::new(
        DryRunMode::parse(Some("dry_run"))?,
        scope,
        OrderInstrument::index_option(specification.clone())?,
        OrderSide::BuyToOpen,
        OrderKind::Market,
        OrderQuantity::contracts(ContractQuantity::new(2)?),
        EvidenceInterval::new(at(1)?, at(10)?)?,
    )?;
    let mut order = Order::new(terms, 8)?;
    apply(&mut run, &mut order, 1, OrderEventKind::Accepted)?;
    let id = OrderEventId::new(run.next_id()?);
    let artifacts = [digest()];
    let contracts = OrderQuantity::contracts(ContractQuantity::new(1)?);
    let mut model = Model {
        calls: 0,
        identity: model_identity()?,
        output: Ok(Some(ProposedFill::new(
            contracts,
            OrderPrice::Premium(OptionPoints::new(101, 2)?),
        ))),
    };
    assert_eq!(
        order.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?),
        Err(OrderError::Price)
    );
    model.output = Ok(Some(ProposedFill::new(
        contracts,
        OrderPrice::Premium(OptionPoints::new(105, 2)?),
    )));
    let fact = order.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?)?;
    order.apply(fact)?;
    assert_eq!(order.quantities(), (Some(contracts), Some(contracts)));

    let other_spec = ReferenceVersion::new(
        specification.id(),
        ReferenceRevision::new(2)?,
        specification.source(),
        specification.known_at(),
        *specification.checksum(),
        *specification.value(),
    );
    let changed_terms = OrderDefinition::new(
        DryRunMode::parse(Some("dry_run"))?,
        scope,
        OrderInstrument::index_option(other_spec)?,
        OrderSide::BuyToOpen,
        OrderKind::Market,
        OrderQuantity::contracts(ContractQuantity::new(2)?),
        EvidenceInterval::new(at(1)?, at(10)?)?,
    )?;
    let mut changed = Order::new(changed_terms, 8)?;
    apply(&mut run, &mut changed, 1, OrderEventKind::Accepted)?;
    let calls = model.calls;
    assert_eq!(
        changed.simulate(&run, &mut model, step(id, 2, &ready, &artifacts)?),
        Err(OrderError::Evidence)
    );
    assert_eq!(model.calls, calls);
    Ok(())
}
