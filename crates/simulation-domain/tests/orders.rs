#[path = "orders/simulation.rs"]
mod simulation;
#[path = "orders/support.rs"]
mod support;

use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::{Run, orders::*};
use support::*;

#[test]
fn dry_run_mode_units_and_pinned_option_lifetimes_fail_closed() -> TestResult {
    for mode in [
        None,
        Some("live"),
        Some("paper"),
        Some("DRY_RUN"),
        Some("dry_run "),
        Some(""),
    ] {
        assert_eq!(DryRunMode::parse(mode), Err(OrderError::Mode));
    }
    assert!(DryRunMode::parse(Some("dry_run")).is_ok());
    assert_eq!(OrderQuantity::shares(0), Err(OrderError::Quantity));
    assert_eq!(
        OrderQuantity::contracts(ContractQuantity::new(u64::MAX)?).parts(),
        (QuantityUnit::Contracts, u64::MAX)
    );
    let evidence = Evidence::trade()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let original = definition(&mut run, OrderSide::BuyToOpen, 10)?;
    let scope = original.scope();
    let mode = DryRunMode::parse(Some("dry_run"))?;
    let spec = option(ExerciseStyle::European, SettlementStyle::Cash)?;
    let instrument = OrderInstrument::index_option(spec)?;
    let terms = OrderKind::Limit(OrderPrice::Premium(OptionPoints::new(105, 2)?));
    let contracts = OrderQuantity::contracts(ContractQuantity::new(2)?);
    let build = |instrument, kind, quantity, lifetime| {
        OrderDefinition::new(
            mode,
            scope,
            instrument,
            OrderSide::BuyToOpen,
            kind,
            quantity,
            lifetime,
        )
    };
    assert!(build(instrument.clone(), terms, contracts, original.lifetime()).is_ok());
    assert_eq!(
        build(instrument.clone(), terms, quantity(2)?, original.lifetime()),
        Err(OrderError::Unit)
    );
    assert_eq!(
        build(
            OrderInstrument::equity(InstrumentId::new(1)?),
            terms,
            contracts,
            original.lifetime()
        ),
        Err(OrderError::Unit)
    );
    assert_eq!(
        build(
            instrument.clone(),
            OrderKind::Limit(price(1)?),
            contracts,
            original.lifetime()
        ),
        Err(OrderError::Unit)
    );
    assert_eq!(
        build(
            instrument.clone(),
            OrderKind::Limit(OrderPrice::Premium(OptionPoints::new(101, 2)?)),
            contracts,
            original.lifetime()
        ),
        Err(OrderError::Price)
    );
    let too_late = EvidenceInterval::new(at(1)?, PreciseInstant::from_unix_parts(10, 1)?)?;
    assert_eq!(
        build(instrument, terms, contracts, too_late),
        Err(OrderError::Specification)
    );
    assert_eq!(
        OrderInstrument::index_option(option(ExerciseStyle::American, SettlementStyle::Cash)?),
        Err(OrderError::Specification)
    );
    let physical = SettlementStyle::Physical(DeliverableReference::new(
        ReferenceId::new(2)?,
        ReferenceRevision::new(1)?,
    ));
    assert_eq!(
        OrderInstrument::index_option(option(ExerciseStyle::European, physical)?),
        Err(OrderError::Specification)
    );
    assert_eq!(
        Order::new(original.clone(), 0).err(),
        Some(OrderError::Capacity)
    );
    assert_eq!(Order::new(original, 4097).err(), Some(OrderError::Capacity));
    Ok(())
}

#[test]
fn future_specifications_and_same_order_submission_conflicts_are_rejected() -> TestResult {
    let evidence = Evidence::trade()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let original = definition(&mut run, OrderSide::BuyToOpen, 10)?;
    let mode = DryRunMode::parse(Some("dry_run"))?;
    let changed = OrderDefinition::new(
        mode,
        original.scope(),
        original.instrument().clone(),
        OrderSide::BuyToOpen,
        original.terms().1,
        quantity(11)?,
        original.lifetime(),
    )?;
    let order = Order::new(original.clone(), 8)?;
    assert_eq!(
        order.retry_submission(&changed),
        Err(OrderError::IdentityConflict)
    );
    let specification = option(ExerciseStyle::European, SettlementStyle::Cash)?;
    let future = ReferenceVersion::new(
        specification.id(),
        specification.revision(),
        specification.source(),
        UtcInstant::from_unix_millis(2000)?,
        *specification.checksum(),
        *specification.value(),
    );
    assert_eq!(
        OrderDefinition::new(
            mode,
            original.scope(),
            OrderInstrument::index_option(future)?,
            OrderSide::BuyToOpen,
            OrderKind::Market,
            OrderQuantity::contracts(ContractQuantity::new(1)?),
            original.lifetime()
        ),
        Err(OrderError::Specification)
    );
    assert_eq!(
        OrderEvent::new(
            OrderEventId::new(run.next_id()?),
            original.scope().identity().3,
            0,
            at(1)?,
            OrderEventKind::Accepted
        ),
        Err(OrderError::Revision)
    );
    Ok(())
}

#[test]
fn cancellation_keeps_partial_fills_and_terminal_races_are_ordered() -> TestResult {
    let evidence = Evidence::trade()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let mut order = Order::new(definition(&mut run, OrderSide::BuyToOpen, 10)?, 32)?;
    let accepted = apply(&mut run, &mut order, 1, OrderEventKind::Accepted)?;
    apply(&mut run, &mut order, 2, fill(3, 99)?)?;
    apply(&mut run, &mut order, 3, OrderEventKind::CancelRequested)?;
    apply(&mut run, &mut order, 3, fill(2, 100)?)?;
    assert_eq!(order.state().0, OrderStatus::CancelPending);
    apply(&mut run, &mut order, 3, OrderEventKind::CancelRejected)?;
    assert_eq!(order.state().0, OrderStatus::PartiallyFilled);
    apply(&mut run, &mut order, 4, OrderEventKind::CancelRequested)?;
    apply(&mut run, &mut order, 4, OrderEventKind::Cancelled)?;
    assert_eq!(order.quantities(), (Some(quantity(5)?), Some(quantity(5)?)));
    assert_eq!(order.apply(accepted)?, ApplyOutcome::Duplicate);
    let losing_fill = event(&mut run, &order, 8, 4, fill(5, 100)?)?;
    unchanged(&mut order, losing_fill, OrderError::State);
    let mut replay = Order::new(order.definition().clone(), 32)?;
    for fact in order.events() {
        replay.apply(*fact)?;
    }
    assert_eq!(replay.state(), order.state());
    assert_eq!(replay.quantities(), order.quantities());
    assert_eq!(replay.events(), order.events());

    let mut winner = Order::new(definition(&mut run, OrderSide::SellToClose, 10)?, 8)?;
    apply(&mut run, &mut winner, 1, OrderEventKind::Accepted)?;
    apply(&mut run, &mut winner, 2, OrderEventKind::CancelRequested)?;
    let full = apply(&mut run, &mut winner, 2, fill(10, 101)?)?;
    assert_eq!(winner.state().0, OrderStatus::Filled);
    let losing_cancel = event(&mut run, &winner, 4, 2, OrderEventKind::Cancelled)?;
    unchanged(&mut winner, losing_cancel, OrderError::State);
    assert_eq!(winner.apply(full)?, ApplyOutcome::Duplicate);
    Ok(())
}

#[test]
fn failures_do_not_mutate_and_exact_duplicates_survive_full_journals() -> TestResult {
    let evidence = Evidence::trade()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let mut order = Order::new(definition(&mut run, OrderSide::BuyToOpen, u64::MAX)?, 4)?;
    assert_eq!(
        order.retry_submission(order.definition())?,
        ApplyOutcome::Duplicate
    );
    let other = definition(&mut run, OrderSide::BuyToOpen, 2)?;
    assert_eq!(
        order.retry_submission(&other),
        Err(OrderError::IdentityConflict)
    );
    let accepted = apply(&mut run, &mut order, 2, OrderEventKind::Accepted)?;
    let conflict = OrderEvent::new(
        accepted.position().0,
        accepted.position().1,
        1,
        at(2)?,
        OrderEventKind::Rejected(OrderRejection::Risk),
    )?;
    unchanged(&mut order, conflict, OrderError::IdentityConflict);
    for (revision, second, kind, error) in [
        (3, 2, fill(1, 100)?, OrderError::RevisionGap),
        (1, 2, fill(1, 100)?, OrderError::Revision),
        (2, 1, fill(1, 100)?, OrderError::Time),
        (2, 2, fill(1, 101)?, OrderError::Price),
        (2, 2, OrderEventKind::Cancelled, OrderError::State),
        (2, 9, OrderEventKind::Expired, OrderError::Time),
        (2, 10, fill(1, 100)?, OrderError::Time),
    ] {
        let fact = event(&mut run, &order, revision, second, kind)?;
        unchanged(&mut order, fact, error);
    }
    let wrong = OrderEvent::new(
        OrderEventId::new(run.next_id()?),
        other.scope().identity().3,
        2,
        at(2)?,
        fill(1, 100)?,
    )?;
    unchanged(&mut order, wrong, OrderError::WrongOrder);
    apply(&mut run, &mut order, 2, fill(u64::MAX - 1, 100)?)?;
    let overflow = event(&mut run, &order, 3, 2, fill(2, 100)?)?;
    unchanged(&mut order, overflow, OrderError::Quantity);
    apply(&mut run, &mut order, 3, OrderEventKind::CancelRequested)?;
    let expired = apply(&mut run, &mut order, 10, OrderEventKind::Expired)?;
    assert_eq!(order.state().0, OrderStatus::Expired);
    assert_eq!(order.quantities().1, Some(quantity(1)?));
    assert_eq!(order.apply(expired)?, ApplyOutcome::Duplicate);
    let fact = event(&mut run, &order, 5, 11, OrderEventKind::Expired)?;
    unchanged(&mut order, fact, OrderError::Capacity);
    Ok(())
}

#[test]
fn rejection_sell_limits_and_payload_redaction() -> TestResult {
    let evidence = Evidence::trade()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let mut rejected = Order::new(definition(&mut run, OrderSide::BuyToOpen, 1)?, 8)?;
    let fact = event(&mut run, &rejected, 1, 1, fill(1, 100)?)?;
    unchanged(&mut rejected, fact, OrderError::State);
    apply(
        &mut run,
        &mut rejected,
        1,
        OrderEventKind::Rejected(OrderRejection::Funding),
    )?;
    assert_eq!(rejected.state().0, OrderStatus::Rejected);
    let mut sell = Order::new(definition(&mut run, OrderSide::SellToClose, 7_654_321)?, 8)?;
    apply(&mut run, &mut sell, 1, OrderEventKind::Accepted)?;
    let fact = event(&mut run, &sell, 2, 1, fill(1, 99)?)?;
    unchanged(&mut sell, fact, OrderError::Price);
    let wrong_unit = OrderEventKind::Filled(FillFact::new(
        ProposedFill::new(
            OrderQuantity::contracts(ContractQuantity::new(1)?),
            price(100)?,
        ),
        model_identity()?,
        digest(),
    ));
    let fact = event(&mut run, &sell, 2, 1, wrong_unit)?;
    unchanged(&mut sell, fact, OrderError::Unit);
    apply(&mut run, &mut sell, 1, fill(7_654_321, 100)?)?;
    assert!(!format!("{sell:?}").contains("7654321"));
    assert!(!format!("{:?}", fill(7_654_321, 7_654_321)?).contains("7654321"));
    Ok(())
}
