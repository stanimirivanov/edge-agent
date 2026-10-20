use super::support::*;
use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::{Run, admission::*, orders::*, risk::UsdCents};

#[test]
fn synthetic_debits_round_up_include_fees_and_never_credit_sale_proceeds() -> TestResult {
    let evidence = Evidence::trade()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let validity = EvidenceInterval::new(at(0)?, at(10)?)?;
    let order = definition(&mut run, OrderSide::BuyToOpen, 3)?;
    let quote = SyntheticReservationQuote::new(
        order.clone(),
        OrderPrice::PerShare(UsdPrice::new(10001, 4)?),
        UsdCents::new(7),
        validity,
        1,
        at(1)?,
    )?;
    assert_eq!(quote.cash()?.get(), 308);
    assert!(quote.validate_at(at(1)?).is_ok());
    assert_eq!(quote.validate_at(at(10)?), Err(ReservationError::Stale));
    assert_eq!(quote.validate_at(at(2)?), Err(ReservationError::Stale));
    let changed = SyntheticReservationQuote::new(
        order,
        quote.protection().0,
        UsdCents::new(8),
        validity,
        1,
        at(1)?,
    )?;
    assert_ne!(quote.fingerprint(), changed.fingerprint());
    assert!(!format!("{quote:?}").contains("10001"));
    let sell = SyntheticReservationQuote::new(
        definition(&mut run, OrderSide::SellToClose, 3)?,
        price(100)?,
        UsdCents::new(7),
        validity,
        2,
        at(1)?,
    )?;
    assert_eq!(sell.cash()?.get(), 7);
    Ok(())
}
#[test]
fn synthetic_option_reservations_use_pinned_multiplier_and_fail_closed_on_overflow() -> TestResult {
    let evidence = Evidence::trade()?;
    let manifest = evidence.manifest()?;
    let mut run = Run::new(&manifest);
    let original = definition(&mut run, OrderSide::BuyToOpen, 1)?;
    let build = |quantity| -> TestResult<OrderDefinition> {
        Ok(OrderDefinition::new(
            DryRunMode::parse(Some("dry_run"))?,
            original.scope(),
            OrderInstrument::index_option(option(ExerciseStyle::European, SettlementStyle::Cash)?)?,
            OrderSide::BuyToOpen,
            OrderKind::Market,
            OrderQuantity::contracts(ContractQuantity::new(quantity)?),
            original.lifetime(),
        )?)
    };
    let validity = original.lifetime();
    let quote = SyntheticReservationQuote::new(
        build(2)?,
        OrderPrice::Premium(OptionPoints::new(105, 2)?),
        UsdCents::new(9),
        validity,
        1,
        at(1)?,
    )?;
    assert_eq!(quote.cash()?.get(), 21009);
    assert_eq!(
        SyntheticReservationQuote::new(
            build(u64::MAX)?,
            OrderPrice::Premium(OptionPoints::new(105, 2)?),
            UsdCents::new(0),
            validity,
            1,
            at(1)?
        ),
        Err(ReservationError::Overflow)
    );
    assert_eq!(
        SyntheticReservationQuote::new(build(1)?, price(1)?, UsdCents::new(0), validity, 1, at(1)?),
        Err(ReservationError::Terms)
    );
    Ok(())
}
