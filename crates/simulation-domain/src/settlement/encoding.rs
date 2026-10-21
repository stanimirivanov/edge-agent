//! Single canonical encoder for policy and synthetic fixing artifacts. Fixed-width
//! big-endian fields and explicit enum tags; no serde feature or Debug dependence.
use super::{FeeBasis, SettlementPolicy, SettlementRounding, SyntheticFinalFixing};

pub(super) fn policy(policy: &SettlementPolicy) -> Vec<u8> {
    let mut bytes = b"EA-SYNTHETIC-SETTLEMENT-POLICY1\0".to_vec();
    let reference = &policy.specification;
    bytes.extend(reference.id().get().to_be_bytes());
    bytes.extend(reference.revision().get().to_be_bytes());
    bytes.extend(reference.source().get().to_be_bytes());
    bytes.extend(reference.known_at().unix_millis().to_be_bytes());
    bytes.extend(reference.checksum());
    let spec = reference.value();
    bytes.extend(spec.id().get().to_be_bytes());
    bytes.extend(spec.underlying().id().get().to_be_bytes());
    let (kind, strike) = spec.terms().payoff();
    // The constructor rejects all but cash/European/index; keep that semantic
    // scope in the versioned prefix instead of accepting unknown variant tags.
    bytes.push(u8::from(
        kind == edgeagent_market_data_domain::OptionKind::Put,
    ));
    for (n, scale) in [
        strike.decimal_parts(),
        spec.terms().quotation().0.decimal_parts(),
        spec.terms().quotation().1.decimal_parts(),
    ] {
        bytes.extend(n.to_be_bytes());
        bytes.extend(scale.to_be_bytes());
    }
    let schedule = spec.schedule();
    for rules in [
        edgeagent_market_data_domain::OptionSchedule::timezone_rules().0,
        edgeagent_market_data_domain::OptionSchedule::timezone_rules().1,
    ] {
        bytes.extend(rules.as_bytes());
        bytes.push(0);
    }
    for at in [
        schedule.trading_bounds().0,
        schedule.trading_bounds().1,
        schedule.expiration(),
        schedule.exercise_cutoff(),
        schedule.settlement(),
        spec.effective().bounds().0,
        spec.effective().bounds().1,
    ] {
        bytes.extend(at.unix_millis().to_be_bytes());
    }
    let (id, revision) = policy.semantics.version();
    bytes.extend(id.get().to_be_bytes());
    bytes.extend(revision.get().to_be_bytes());
    bytes.extend(policy.source.get().to_be_bytes());
    for at in [policy.fixing_at, policy.known_by] {
        instant(&mut bytes, at);
    }
    bytes.push(match policy.rounding {
        SettlementRounding::Down => 0,
        SettlementRounding::Up => 1,
        SettlementRounding::NearestEven => 2,
    });
    bytes.push(match policy.charges.basis {
        FeeBasis::EveryContract => 0,
        FeeBasis::InTheMoney => 1,
    });
    bytes.extend(policy.charges.cents.to_be_bytes());
    bytes
}
pub(super) fn fixing(fixing: &SyntheticFinalFixing) -> Vec<u8> {
    let mut bytes = b"EA-SYNTHETIC-FINAL-FIXING1\0".to_vec();
    bytes.extend(fixing.policy.bytes());
    bytes.extend(fixing.id.get().to_be_bytes());
    instant(&mut bytes, fixing.known_at);
    let (n, scale) = fixing.level.decimal_parts();
    bytes.extend(n.to_be_bytes());
    bytes.extend(scale.to_be_bytes());
    bytes
}
fn instant(bytes: &mut Vec<u8>, at: edgeagent_market_data_domain::PreciseInstant) {
    let (seconds, nanos) = at.unix_parts();
    bytes.extend(seconds.to_be_bytes());
    bytes.extend(nanos.to_be_bytes());
}
