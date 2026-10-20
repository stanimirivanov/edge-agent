#[allow(dead_code)]
#[path = "orders/support.rs"]
mod support;
use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::{ArtifactDigest, Run, execution::*, orders::*};
use std::time::Duration;
use support::*;

fn profile() -> TestResult<ExecutionProfile> {
    Ok(ExecutionProfile::new(
        model_identity()?.0,
        2_500,
        500_000,
        250_000,
        Duration::from_secs(1),
        Duration::from_secs(2),
    )?)
}
#[test]
fn mandatory_profile_parameters_pin_fees_participation_and_virtual_delays() -> TestResult {
    let p = profile()?;
    assert_eq!(p.identity().1, ArtifactDigest::of(&p.canonical_bytes()));
    assert!(
        ExecutionProfile::new(model_identity()?.0, 0, 0, 0, Duration::ZERO, Duration::ZERO)
            .is_err()
    );
    assert!(
        ExecutionProfile::new(
            model_identity()?.0,
            10_001,
            0,
            0,
            Duration::ZERO,
            Duration::ZERO
        )
        .is_err()
    );
    assert!(
        ExecutionProfile::new(
            model_identity()?.0,
            1,
            0,
            0,
            Duration::from_secs(86_401),
            Duration::ZERO
        )
        .is_err()
    );
    assert_eq!(p.capacity(99, 1)?, 2); // floor(9.9 * 25%)
    assert_eq!(
        (
            p.fee_cents(0)?,
            p.fee_cents(1)?,
            p.fee_cents(2)?,
            p.fee_cents(3)?
        ),
        (0, 1, 1, 2)
    );
    assert_eq!(p.submission_due(at(1)?)?, at(2)?);
    assert_eq!(p.cancellation_due(at(2)?)?, at(4)?);
    let changed = ExecutionProfile::new(
        model_identity()?.0,
        2_500,
        500_001,
        250_000,
        Duration::from_secs(1),
        Duration::from_secs(2),
    )?;
    assert_ne!(p.identity(), changed.identity());
    Ok(())
}
#[test]
fn consumed_trade_capacity_is_shared_and_limit_and_ceiling_protected() -> TestResult {
    let evidence = Evidence::trade()?;
    let ready = evidence.ready()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let order = definition(&mut run, OrderSide::BuyToOpen, 3)?;
    assert!(
        ExecutableLiquidity::from_ready(&run, &order, profile()?, &ready, &[digest()]).is_err()
    );
    run.next_input();
    let liquidity = ExecutableLiquidity::from_ready(&run, &order, profile()?, &ready, &[digest()])?;
    assert_eq!(liquidity.budget().2, 2);
    assert_eq!(format!("{liquidity:?}"), "ExecutableLiquidity(..)");
    assert!(LiquidityExecution::new(liquidity.clone(), 3, price(100)?, at(1)?).is_err());
    assert_eq!(
        liquidity
            .propose(&order, quantity(3)?, 0, price(100)?)?
            .ok_or("no fill")?
            .values()
            .0,
        quantity(2)?
    );
    assert!(
        liquidity
            .propose(&order, quantity(3)?, 2, price(100)?)?
            .is_none()
    );
    assert!(
        liquidity
            .propose(&order, quantity(3)?, 3, price(100)?)
            .is_err()
    );
    assert!(
        liquidity
            .propose(&order, quantity(3)?, 0, price(99)?)?
            .is_none()
    );
    assert!(
        ExecutableLiquidity::from_ready(
            &run,
            &order,
            profile()?,
            &ready,
            &[ArtifactDigest::of(b"changed")]
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn registered_port_retains_pre_activation_no_fill_then_executes_at_virtual_due() -> TestResult {
    let evidence = Evidence::trade()?;
    let ready = evidence.ready()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let mut order = Order::new(definition(&mut run, OrderSide::BuyToOpen, 3)?, 8)?;
    run.next_input();
    apply(&mut run, &mut order, 1, OrderEventKind::Accepted)?;
    let market =
        ExecutableLiquidity::from_ready(&run, order.definition(), profile()?, &ready, &[digest()])?;
    let mut model = LiquidityExecution::new(market, 0, price(100)?, at(1)?)?;
    assert_eq!(format!("{model:?}"), "LiquidityExecution(..)");
    let first = OrderEventId::new(run.next_id()?);
    let fact = order.simulate(
        &run,
        &mut model,
        SimulationStep::new(
            first,
            2,
            &ready,
            profile()?.identity(),
            digest(),
            &[digest()],
        )?,
    )?;
    assert!(matches!(fact.effect().1, OrderEventKind::NoFill { .. }));
    order.apply(fact)?;
    let retained = order.simulate(
        &run,
        &mut model,
        SimulationStep::new(
            first,
            2,
            &ready,
            profile()?.identity(),
            digest(),
            &[digest()],
        )?,
    )?;
    assert_eq!(retained, fact);
    run.advance_to(at(2)?)?;
    let ready = evidence.ready_at(2)?;
    let id = OrderEventId::new(run.next_id()?);
    let fact = order.simulate(
        &run,
        &mut model,
        SimulationStep::new(id, 3, &ready, profile()?.identity(), digest(), &[digest()])?,
    )?;
    assert!(matches!(fact.effect().1, OrderEventKind::Filled(_)));
    order.apply(fact)?;
    assert_eq!(order.quantities().0.ok_or("not filled")?.parts().1, 2);
    Ok(())
}
#[test]
fn bars_cannot_become_intrabar_execution_evidence() -> TestResult {
    let coverage = BarCoverage::Intraday;
    let evidence = Evidence::new(
        1,
        ObservationValue::Bar(RawBar::new(
            EvidenceInterval::new(at(-1)?, at(0)?)?,
            coverage,
            [UsdPrice::new(100, 0)?; 4],
            ShareVolume::new(100, 0)?,
        )?),
        EvidenceKind::RawBar(coverage),
        MarketScope::Consolidated,
    )?;
    let manifest = evidence.manifest()?;
    let ready = evidence.ready()?;
    let mut run = Run::new(&manifest);
    let order = definition(&mut run, OrderSide::BuyToOpen, 1)?;
    run.next_input();
    assert!(
        ExecutableLiquidity::from_ready(&run, &order, profile()?, &ready, &[digest()]).is_err()
    );
    Ok(())
}
#[test]
fn split_fills_round_once_balance_postings_and_preserve_average_cost_remainders() -> TestResult {
    let evidence = Evidence::trade()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let order = definition(&mut run, OrderSide::BuyToOpen, 3)?;
    let p = ExecutionProfile::new(
        model_identity()?.0,
        10_000,
        500_000,
        100_000,
        Duration::ZERO,
        Duration::ZERO,
    )?;
    let tiny = OrderPrice::PerShare(UsdPrice::new(33333, 5)?);
    let mut position = PositionCost::new(0, 0)?;
    let mut totals = (0, 0, 0, 0);
    let mut cash = 0;
    let mut fees = 0;
    for expected_gross in [34, 67, 100] {
        let fill =
            FillAccounting::calculate(&order, p, tiny, 1, (totals.0, totals.1), 1, position)?;
        totals = fill.totals();
        let effects = fill.effects();
        position = effects.0;
        assert_eq!(effects.1.iter().sum::<i128>(), 0);
        assert_eq!(totals.2, expected_gross);
        cash += effects.1[0];
        fees += effects.1[3];
    }
    assert_eq!((cash, fees, position.values()), (-101, 1, (3, 100)));
    let whole = FillAccounting::calculate(&order, p, tiny, 3, (0, 0), 1, PositionCost::new(0, 0)?)?;
    assert_eq!(whole.totals(), totals);
    assert!(
        FillAccounting::calculate(&order, p, tiny, 1, (0, 0), 0, PositionCost::new(0, 0)?).is_err()
    );
    let sell = definition(&mut run, OrderSide::SellToClose, 3)?;
    let mut previous = (0, 0);
    let mut released = 0;
    for count in 1..=3 {
        let fill = FillAccounting::calculate(&sell, p, price(100)?, 1, previous, 1, position)?;
        previous = (fill.totals().0, fill.totals().1);
        let (next, postings) = fill.effects();
        position = next;
        assert_eq!(postings.iter().sum::<i128>(), 0);
        released -= postings[1];
        assert_eq!(position.values().0, 3 - count);
    }
    assert_eq!((released, position.values()), (100, (0, 0)));
    assert!(PositionCost::new(0, 1).is_err());
    Ok(())
}
#[test]
fn journal_restore_replays_partial_cancel_and_rejects_corruption_or_wrong_catalog() -> TestResult {
    let evidence = Evidence::trade()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let mut order = Order::new(definition(&mut run, OrderSide::BuyToOpen, 3)?, 8)?;
    apply(&mut run, &mut order, 1, OrderEventKind::Accepted)?;
    let partial = apply(&mut run, &mut order, 2, fill(1, 100)?)?;
    apply(&mut run, &mut order, 2, OrderEventKind::CancelRequested)?;
    apply(
        &mut run,
        &mut order,
        3,
        OrderEventKind::NoFill {
            model: model_identity()?,
            evidence: digest(),
        },
    )?;
    apply(&mut run, &mut order, 4, OrderEventKind::Cancelled)?;
    let bytes = order.journal_bytes(digest());
    let mut restored = Order::restore_journal(order.definition().clone(), 8, digest(), &bytes)?;
    assert_eq!(restored.state(), order.state());
    assert_eq!(restored.events(), order.events());
    assert_eq!(restored.apply(partial)?, ApplyOutcome::Duplicate);
    assert_eq!(FillAccounting::retained_totals(&restored, profile()?)?.1, 1);
    assert!(
        Order::restore_journal(
            order.definition().clone(),
            8,
            ArtifactDigest::of(b"other"),
            &bytes
        )
        .is_err()
    );
    for length in [0, 31, 76, bytes.len() - 1] {
        assert!(
            Order::restore_journal(order.definition().clone(), 8, digest(), &bytes[..length])
                .is_err()
        );
    }
    assert!(Order::restore_journal(order.definition().clone(), 4, digest(), &bytes).is_err());
    let mut corrupt = bytes;
    if let Some(byte) = corrupt.get_mut(40) {
        *byte ^= 1;
    }
    assert!(Order::restore_journal(order.definition().clone(), 8, digest(), &corrupt).is_err());
    Ok(())
}
#[test]
fn option_liquidity_uses_ask_for_buy_bid_for_sell_and_unknown_size_abstains() -> TestResult {
    let spec = option(ExerciseStyle::European, SettlementStyle::Cash)?;
    for (buy, unknown) in [(true, false), (false, false), (true, true)] {
        let bid = QuoteSide::new(
            OptionPoints::new(55, 2)?,
            Some(DisplayedContracts::new(4)),
            at(0)?,
        );
        let ask = QuoteSide::new(
            OptionPoints::new(60, 2)?,
            if unknown {
                None
            } else {
                Some(DisplayedContracts::new(4))
            },
            at(0)?,
        );
        let evidence = Evidence::new(
            10,
            ObservationValue::OptionQuote(OptionQuoteSnapshot::new(&spec, bid, ask)?),
            EvidenceKind::OptionSnapshot,
            MarketScope::Consolidated,
        )?;
        let ready = evidence.ready()?;
        let manifest = evidence.manifest()?;
        let mut run = Run::new(&manifest);
        let order = OrderDefinition::new(
            DryRunMode::parse(Some("dry_run"))?,
            OrderScope::new(
                manifest.digest(),
                edgeagent_simulation_domain::risk::RiskAccountId::new(1)?,
                edgeagent_simulation_domain::risk::RiskStrategyId::new(1)?,
                OrderId::new(run.next_id()?),
            ),
            OrderInstrument::index_option(spec.clone())?,
            if buy {
                OrderSide::BuyToOpen
            } else {
                OrderSide::SellToClose
            },
            OrderKind::Market,
            OrderQuantity::contracts(ContractQuantity::new(2)?),
            EvidenceInterval::new(at(1)?, at(10)?)?,
        )?;
        run.next_input();
        let market =
            ExecutableLiquidity::from_ready(&run, &order, profile()?, &ready, &[digest()])?;
        let proposed = market.propose(
            &order,
            order.terms().2,
            0,
            OrderPrice::Premium(OptionPoints::new(60, 2)?),
        )?;
        if unknown {
            assert!(proposed.is_none());
        } else {
            let (qty, price) = proposed.ok_or("no option fill")?.values();
            assert_eq!(qty.parts().1, 1);
            assert_eq!(
                price,
                OrderPrice::Premium(OptionPoints::new(if buy { 60 } else { 55 }, 2)?)
            );
        }
    }
    Ok(())
}
