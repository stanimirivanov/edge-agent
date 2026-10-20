use super::{ReservationError, SyntheticReservationQuote};
use crate::orders::{OrderKind, OrderPrice, OrderSide, QuantityUnit};
use edgeagent_market_data_domain::{OptionKind, PreciseInstant};

pub(super) fn quote(quote: &SyntheticReservationQuote) -> Result<Vec<u8>, ReservationError> {
    let mut out = b"EA-SYNTHETIC-RESERVATION1/CEIL-USD-CENTS\0".to_vec();
    let definition = &quote.order;
    let (run, account, strategy, order) = definition.scope().identity();
    out.extend(run.bytes());
    out.extend(account.get().to_be_bytes());
    out.extend(strategy.get().to_be_bytes());
    out.extend(order.bytes());
    out.extend(definition.instrument().id().get().to_be_bytes());
    if let Some(spec) = definition.instrument().option_specification() {
        out.push(1);
        out.extend(spec.id().get().to_be_bytes());
        out.extend(spec.revision().get().to_be_bytes());
        out.extend(spec.source().get().to_be_bytes());
        out.extend(spec.known_at().unix_millis().to_be_bytes());
        out.extend(spec.checksum());
        let value = spec.value();
        out.extend(value.underlying().id().get().to_be_bytes());
        let terms = value.terms();
        out.push(match terms.payoff().0 {
            OptionKind::Call => 0,
            OptionKind::Put => 1,
            _ => return Err(ReservationError::Terms),
        });
        decimal(terms.payoff().1.decimal_parts(), &mut out);
        decimal(terms.quotation().0.decimal_parts(), &mut out);
        decimal(terms.quotation().1.decimal_parts(), &mut out);
        // Construction admits only USD/cash/European/index; encode these fixed
        // semantics in the version prefix rather than platform-specific Debug.
        let schedule = value.schedule();
        for instant in [
            schedule.trading_bounds().0,
            schedule.trading_bounds().1,
            schedule.expiration(),
            schedule.exercise_cutoff(),
            schedule.settlement(),
            value.effective().bounds().0,
            value.effective().bounds().1,
        ] {
            out.extend(instant.unix_millis().to_be_bytes());
        }
        let (zone, revision) = edgeagent_market_data_domain::OptionSchedule::timezone_rules();
        out.extend((zone.len() as u64).to_be_bytes());
        out.extend(zone.as_bytes());
        out.extend((revision.len() as u64).to_be_bytes());
        out.extend(revision.as_bytes());
    } else {
        out.push(0);
    }
    let (side, kind, quantity) = definition.terms();
    out.push(match side {
        OrderSide::BuyToOpen => 0,
        OrderSide::SellToClose => 1,
    });
    match kind {
        OrderKind::Market => out.push(0),
        OrderKind::Limit(limit) => {
            out.push(1);
            price(limit, &mut out);
        }
    }
    out.push(match quantity.parts().0 {
        QuantityUnit::Shares => 0,
        QuantityUnit::Contracts => 1,
    });
    out.extend(quantity.parts().1.to_be_bytes());
    for instant in [
        definition.lifetime().bounds().0,
        definition.lifetime().bounds().1,
        quote.validity.bounds().0,
        quote.validity.bounds().1,
        quote.at,
    ] {
        time(instant, &mut out);
    }
    price(quote.ceiling, &mut out);
    out.extend(quote.fees.get().to_be_bytes());
    out.extend(quote.sequence.to_be_bytes());
    Ok(out)
}
fn decimal((coefficient, scale): (i128, u32), out: &mut Vec<u8>) {
    out.extend(coefficient.to_be_bytes());
    out.extend(scale.to_be_bytes());
}
fn price(value: OrderPrice, out: &mut Vec<u8>) {
    match value {
        OrderPrice::PerShare(value) => {
            out.push(0);
            decimal(value.decimal_parts(), out);
        }
        OrderPrice::Premium(value) => {
            out.push(1);
            decimal(value.decimal_parts(), out);
        }
    }
}
fn time(value: PreciseInstant, out: &mut Vec<u8>) {
    let (seconds, nanos) = value.unix_parts();
    out.extend(seconds.to_be_bytes());
    out.extend(nanos.to_be_bytes());
}
