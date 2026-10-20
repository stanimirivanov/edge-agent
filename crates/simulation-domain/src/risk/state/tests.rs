use super::*;
use crate::risk::{RiskLimits, RiskStrategyId, RiskUsage, StrategyValuation, UsdCents};
use edgeagent_market_data_domain::{EvidenceInterval, ReferenceId, ReferenceRevision, SessionDate};
use std::time::Duration;

#[test]
fn exhausted_revision_never_partially_applies_observe_reset_or_rollover()
-> Result<(), Box<dyn std::error::Error>> {
    let start = PreciseInstant::from_unix_parts(0, 0)?;
    let end = PreciseInstant::from_unix_parts(100, 0)?;
    let revision = SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?);
    let session = RiskSession::new(
        SessionDate::new(1970, 1, 1)?,
        EvidenceInterval::new(start, end)?,
        revision,
    );
    let strategy = RiskStrategyId::new(1)?;
    let actor = RiskOperatorId::new(1)?;
    let account = RiskAccountId::new(1)?;
    let limits = RiskLimits::new(UsdCents::new(1), UsdCents::new(1), 1);
    let policy = RiskPolicy::new(
        revision,
        Duration::from_secs(1),
        limits,
        &[(strategy, limits)],
        &[actor],
    )?;
    let mut state = RiskState::new(account, policy, session, start)?;
    state.revision = u64::MAX;
    let snapshot = RiskSnapshot::new(
        account,
        session,
        &[StrategyValuation::new(
            strategy,
            RiskUsage::new(UsdCents::new(0), UsdCents::new(0), UsdCents::new(0), 0)?,
            start,
        )],
    )?;
    assert_eq!(
        state.observe(u64::MAX, &snapshot, start),
        Err(RiskError::Overflow)
    );
    state.breach = Some(RiskReason::AccountLoss);
    assert_eq!(
        state.reset(u64::MAX, actor, &snapshot, start),
        Err(RiskError::Overflow)
    );
    let next_start = PreciseInstant::from_unix_parts(86400, 0)?;
    let next = RiskSession::new(
        SessionDate::new(1970, 1, 2)?,
        EvidenceInterval::new(next_start, PreciseInstant::from_unix_parts(86500, 0)?)?,
        revision,
    );
    assert_eq!(
        state.rollover(u64::MAX, actor, next, next_start),
        Err(RiskError::Overflow)
    );
    assert_eq!(state.status(), (u64::MAX, Some(RiskReason::AccountLoss)));
    assert_eq!(state.clock, start);
    assert_eq!(state.session, session);
    Ok(())
}
