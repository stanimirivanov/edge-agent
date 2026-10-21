//! Versioned, fixed-width encoding. Retain the actual evidence/assumption bytes
//! outside this calculator; a digest is neither data availability nor attestation.
use super::AnalyticsInputs;
pub(super) fn inputs(input: &AnalyticsInputs) -> Vec<u8> {
    let mut b = b"EA-BSM-ACT365F-LIBM0.2.16-BISECTION1\0".to_vec();
    let r = &input.reference;
    let spec = r.value();
    b.extend(r.id().get().to_be_bytes());
    b.extend(r.revision().get().to_be_bytes());
    b.extend(r.source().get().to_be_bytes());
    b.extend(r.known_at().unix_millis().to_be_bytes());
    b.extend(r.checksum());
    b.extend(spec.id().get().to_be_bytes());
    b.extend(spec.underlying().id().get().to_be_bytes());
    b.push(u8::from(
        spec.terms().payoff().0 == edgeagent_market_data_domain::OptionKind::Put,
    ));
    for (n, s) in [
        spec.terms().payoff().1.decimal_parts(),
        spec.terms().quotation().0.decimal_parts(),
        spec.terms().quotation().1.decimal_parts(),
        input.level.decimal_parts(),
    ] {
        b.extend(n.to_be_bytes());
        b.extend(s.to_be_bytes());
    }
    let schedule = spec.schedule();
    for time in [
        schedule.trading_bounds().0,
        schedule.trading_bounds().1,
        schedule.expiration(),
        schedule.exercise_cutoff(),
        schedule.settlement(),
        spec.effective().bounds().0,
        spec.effective().bounds().1,
    ] {
        b.extend(time.unix_millis().to_be_bytes());
    }
    for rule in [
        edgeagent_market_data_domain::OptionSchedule::timezone_rules().0,
        edgeagent_market_data_domain::OptionSchedule::timezone_rules().1,
    ] {
        b.extend(rule.as_bytes());
        b.push(0);
    }
    let (s, n) = input.at.unix_parts();
    b.extend(s.to_be_bytes());
    b.extend(n.to_be_bytes());
    b.push(match input.basis {
        super::YearBasis::Actual365Fixed => 0,
    });
    for rate in [input.rates.risk_free, input.rates.yield_rate] {
        b.extend(rate.to_bits().to_be_bytes());
    }
    b.extend(input.rates.assumptions.bytes());
    b.extend(input.evidence.bytes());
    b
}
