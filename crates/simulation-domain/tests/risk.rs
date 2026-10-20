use edgeagent_market_data_domain::{
    EvidenceInterval, PreciseInstant, ReferenceId, ReferenceRevision, SemanticReference,
    SessionDate,
};
use edgeagent_simulation_domain::risk::{
    RiskAccountId, RiskDecision, RiskError, RiskIncrease, RiskLimits, RiskOperatorId, RiskPolicy,
    RiskReason, RiskRecoveryKind, RiskSession, RiskSnapshot, RiskState, RiskStrategyId, RiskUsage,
    StrategyValuation, UsdCents,
};
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Value<T> = Result<T, Box<dyn std::error::Error>>;
fn portable<T: Send + Sync>() {}
fn at(seconds: i64) -> Value<PreciseInstant> {
    Ok(PreciseInstant::from_unix_parts(seconds, 0)?)
}
fn rules() -> Value<SemanticReference> {
    Ok(SemanticReference::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(1)?,
    ))
}
fn session(day: u32) -> Value<RiskSession> {
    let start = i64::from(day - 1) * 86400;
    Ok(RiskSession::new(
        SessionDate::new(1970, 1, day)?,
        EvidenceInterval::new(at(start)?, at(start + 100)?)?,
        rules()?,
    ))
}
fn caps(gross: u64, loss: u64, orders: u64) -> RiskLimits {
    RiskLimits::new(UsdCents::new(gross), UsdCents::new(loss), orders)
}
fn policy(account: RiskLimits, strategy: RiskLimits) -> Value<RiskPolicy> {
    Ok(RiskPolicy::new(
        rules()?,
        Duration::from_secs(10),
        account,
        &[
            (RiskStrategyId::new(1)?, strategy),
            (RiskStrategyId::new(2)?, strategy),
        ],
        &[RiskOperatorId::new(7)?],
    )?)
}
fn standard() -> Value<RiskPolicy> {
    policy(caps(100, 20, 4), caps(100, 20, 4))
}
fn state(policy: RiskPolicy) -> Value<RiskState> {
    Ok(RiskState::new(
        RiskAccountId::new(1)?,
        policy,
        session(1)?,
        at(0)?,
    )?)
}
fn row(id: u128, usage: [u64; 4], seconds: i64) -> Value<StrategyValuation> {
    Ok(StrategyValuation::new(
        RiskStrategyId::new(id)?,
        RiskUsage::new(
            UsdCents::new(usage[0]),
            UsdCents::new(usage[1]),
            UsdCents::new(usage[2]),
            usage[3],
        )?,
        at(seconds)?,
    ))
}
fn snapshot(first: StrategyValuation, second: StrategyValuation) -> Value<RiskSnapshot> {
    Ok(RiskSnapshot::new(
        RiskAccountId::new(1)?,
        session(1)?,
        &[first, second],
    )?)
}
fn healthy() -> Value<RiskSnapshot> {
    snapshot(row(1, [10, 0, 0, 1], 10)?, row(2, [10, 0, 0, 1], 10)?)
}

#[test]
fn durable_risk_checkpoint_preserves_breach_revision_and_rejects_other_contexts() -> TestResult {
    let account = RiskAccountId::new(1)?;
    let mut original = state(standard()?)?;
    let breached = snapshot(row(1, [101, 0, 0, 0], 10)?, row(2, [0, 0, 0, 0], 10)?)?;
    assert_eq!(
        original.observe(1, &breached, at(10)?)?,
        RiskDecision::Denied(RiskReason::AccountGross)
    );
    let bytes = original.checkpoint();
    let mut restored = RiskState::restore(account, standard()?, session(1)?, &bytes)?;
    assert_eq!(restored.status(), original.status());
    assert_eq!(
        restored.observe(restored.status().0, &healthy()?, at(10)?)?,
        RiskDecision::Denied(RiskReason::AccountGross)
    );
    assert!(RiskState::restore(RiskAccountId::new(2)?, standard()?, session(1)?, &bytes).is_err());
    assert!(RiskState::restore(account, standard()?, session(2)?, &bytes).is_err());
    assert!(
        RiskState::restore(
            account,
            policy(caps(101, 20, 4), caps(100, 20, 4))?,
            session(1)?,
            &bytes
        )
        .is_err()
    );
    for length in 0..bytes.len() {
        assert!(RiskState::restore(account, standard()?, session(1)?, &bytes[..length]).is_err());
    }
    for offset in 0..bytes.len() {
        let mut corrupt = bytes.clone();
        corrupt[offset] ^= 1;
        assert!(RiskState::restore(account, standard()?, session(1)?, &corrupt).is_err());
    }
    Ok(())
}

#[test]
fn inclusive_limits_sum_positions_and_reservations_across_all_strategies() -> TestResult {
    let first = row(1, [40, 10, 10, 2], 10)?;
    let second = row(2, [40, 10, 10, 2], 10)?;
    let mut original = state(standard()?)?;
    let mut reordered = state(standard()?)?;
    assert_eq!(
        original.observe(1, &snapshot(first, second)?, at(10)?)?,
        RiskDecision::Permitted
    );
    assert_eq!(
        reordered.observe(1, &snapshot(second, first)?, at(10)?)?,
        RiskDecision::Permitted
    );
    assert_eq!(original.status(), reordered.status());
    let reserved = snapshot(row(1, [40, 11, 10, 2], 10)?, second)?;
    assert_eq!(
        original.observe(2, &reserved, at(10)?)?,
        RiskDecision::Denied(RiskReason::AccountGross)
    );
    Ok(())
}

#[test]
fn account_limits_cannot_be_overridden_and_strategy_limits_are_layered_on_top() -> TestResult {
    let cases = [
        (
            caps(100, 50, 8),
            caps(100, 50, 8),
            [60, 0, 0, 1],
            [60, 0, 0, 1],
            RiskReason::AccountGross,
        ),
        (
            caps(100, 50, 8),
            caps(100, 50, 8),
            [0, 0, 30, 0],
            [0, 0, 30, 0],
            RiskReason::AccountLoss,
        ),
        (
            caps(100, 50, 8),
            caps(100, 50, 8),
            [0, 0, 0, 5],
            [0, 0, 0, 5],
            RiskReason::AccountOrders,
        ),
        (
            caps(1000, 100, 100),
            caps(100, 50, 8),
            [101, 0, 0, 0],
            [0; 4],
            RiskReason::StrategyGross(RiskStrategyId::new(1)?),
        ),
        (
            caps(1000, 100, 100),
            caps(100, 50, 8),
            [0, 0, 51, 0],
            [0; 4],
            RiskReason::StrategyLoss(RiskStrategyId::new(1)?),
        ),
        (
            caps(1000, 100, 100),
            caps(100, 50, 8),
            [0, 0, 0, 9],
            [0; 4],
            RiskReason::StrategyOrders(RiskStrategyId::new(1)?),
        ),
    ];
    for (account, strategy, first, second, reason) in cases {
        let mut state = state(policy(account, strategy)?)?;
        assert_eq!(
            state.observe(
                1,
                &snapshot(row(1, first, 10)?, row(2, second, 10)?)?,
                at(10)?
            )?,
            RiskDecision::Denied(reason)
        );
        assert_eq!(state.status(), (2, Some(reason)));
    }
    Ok(())
}

#[test]
fn prospective_denial_does_not_latch_but_current_breach_survives_improvement() -> TestResult {
    let mut state = state(standard()?)?;
    let healthy = healthy()?;
    let increase = RiskIncrease::new(RiskStrategyId::new(1)?, UsdCents::new(81), 0);
    assert_eq!(
        state.check_increase(1, &healthy, at(10)?, increase)?,
        RiskDecision::Denied(RiskReason::AccountGross)
    );
    assert_eq!(state.status(), (2, None));
    assert_eq!(
        state.observe(2, &healthy, at(10)?)?,
        RiskDecision::Permitted
    );
    let breached = snapshot(row(1, [101, 0, 0, 1], 10)?, row(2, [0; 4], 10)?)?;
    assert_eq!(
        state.observe(3, &breached, at(10)?)?,
        RiskDecision::Denied(RiskReason::AccountGross)
    );
    assert_eq!(
        state.observe(4, &healthy, at(10)?)?,
        RiskDecision::Denied(RiskReason::AccountGross)
    );
    assert_eq!(state.status(), (5, Some(RiskReason::AccountGross)));
    Ok(())
}

#[test]
fn missing_duplicate_unknown_and_wrong_scope_inputs_cannot_hide_account_usage() -> TestResult {
    let first = row(1, [0; 4], 10)?;
    let second = row(2, [0; 4], 10)?;
    assert_eq!(
        RiskSnapshot::new(RiskAccountId::new(1)?, session(1)?, &[first, first]).err(),
        Some(RiskError::Configuration)
    );
    assert_eq!(
        RiskSnapshot::new(RiskAccountId::new(1)?, session(1)?, &[]).err(),
        Some(RiskError::Configuration)
    );
    assert_eq!(
        RiskSnapshot::new(RiskAccountId::new(1)?, session(1)?, &vec![first; 33]).err(),
        Some(RiskError::Configuration)
    );
    let mut state = state(standard()?)?;
    for bad in [
        RiskSnapshot::new(RiskAccountId::new(1)?, session(1)?, &[first])?,
        snapshot(first, row(3, [0; 4], 10)?)?,
        RiskSnapshot::new(RiskAccountId::new(2)?, session(1)?, &[first, second])?,
        RiskSnapshot::new(RiskAccountId::new(1)?, session(2)?, &[first, second])?,
    ] {
        assert_eq!(state.observe(1, &bad, at(10)?), Err(RiskError::Scope));
        assert_eq!(state.status(), (1, None));
    }
    assert_eq!(
        state.check_increase(
            1,
            &healthy()?,
            at(10)?,
            RiskIncrease::new(RiskStrategyId::new(3)?, UsdCents::new(0), 0)
        ),
        Err(RiskError::Scope)
    );
    assert_eq!(state.status(), (1, None));
    Ok(())
}

#[test]
fn stale_future_and_exact_nanosecond_freshness_are_fail_closed() -> TestResult {
    let snapshot = snapshot(row(1, [0; 4], 0)?, row(2, [0; 4], 10)?)?;
    let mut state = state(standard()?)?;
    assert_eq!(
        state.observe(1, &snapshot, at(10)?)?,
        RiskDecision::Permitted
    );
    let expired = PreciseInstant::from_unix_parts(10, 1)?;
    assert_eq!(
        state.observe(2, &snapshot, expired)?,
        RiskDecision::Denied(RiskReason::StaleValuation)
    );
    let future = healthy()?;
    assert_eq!(state.observe(3, &future, at(9)?), Err(RiskError::Clock));
    assert_eq!(state.status(), (3, Some(RiskReason::StaleValuation)));
    let future = super_future()?;
    assert_eq!(state.observe(3, &future, expired), Err(RiskError::Clock));
    Ok(())
}
fn super_future() -> Value<RiskSnapshot> {
    snapshot(row(1, [0; 4], 11)?, row(2, [0; 4], 10)?)
}

#[test]
fn reset_requires_current_revision_authorized_actor_and_healthy_evidence() -> TestResult {
    let policy = standard()?;
    let fingerprint = policy.fingerprint();
    let mut state = state(policy)?;
    let breached = snapshot(row(1, [101, 0, 0, 1], 10)?, row(2, [0; 4], 10)?)?;
    state.observe(1, &breached, at(10)?)?;
    assert_eq!(
        state.reset(2, RiskOperatorId::new(99)?, &healthy()?, at(10)?),
        Err(RiskError::Unauthorized)
    );
    assert_eq!(
        state.reset(1, RiskOperatorId::new(7)?, &healthy()?, at(10)?),
        Err(RiskError::Revision)
    );
    assert_eq!(
        state.reset(2, RiskOperatorId::new(7)?, &breached, at(10)?),
        Err(RiskError::Reset)
    );
    assert_eq!(state.status(), (2, Some(RiskReason::AccountGross)));
    let receipt = state.reset(2, RiskOperatorId::new(7)?, &healthy()?, at(10)?)?;
    assert_eq!(
        receipt.audit(),
        (
            RiskOperatorId::new(7)?,
            3,
            rules()?,
            RiskRecoveryKind::Reset
        )
    );
    assert_eq!(receipt.policy_fingerprint(), fingerprint);
    assert_eq!(receipt.context(), (at(10)?, session(1)?));
    assert_eq!(state.status(), (3, None));
    assert_eq!(
        state.reset(3, RiskOperatorId::new(7)?, &healthy()?, at(10)?),
        Err(RiskError::Reset)
    );
    assert_eq!(
        state.reset(2, RiskOperatorId::new(7)?, &healthy()?, at(10)?),
        Err(RiskError::Revision)
    );
    Ok(())
}

#[test]
fn session_end_rollover_keeps_latch_and_does_not_grant_a_new_exposure_budget() -> TestResult {
    let mut state = state(standard()?)?;
    assert_eq!(
        state.observe(1, &healthy()?, at(100)?)?,
        RiskDecision::Denied(RiskReason::SessionClosed)
    );
    assert_eq!(
        state
            .rollover(2, RiskOperatorId::new(99)?, session(2)?, at(86400)?)
            .err(),
        Some(RiskError::Unauthorized)
    );
    assert_eq!(
        state
            .rollover(2, RiskOperatorId::new(7)?, session(2)?, at(99)?)
            .err(),
        Some(RiskError::Clock)
    );
    assert_eq!(
        state
            .rollover(2, RiskOperatorId::new(7)?, session(1)?, at(100)?)
            .err(),
        Some(RiskError::Session)
    );
    let receipt = state.rollover(2, RiskOperatorId::new(7)?, session(2)?, at(86400)?)?;
    assert_eq!(receipt.audit().3, RiskRecoveryKind::Rollover);
    assert_eq!(state.status(), (3, Some(RiskReason::SessionClosed)));
    assert_eq!(
        state.observe(3, &healthy()?, at(86400)?),
        Err(RiskError::Scope)
    );
    let carried = RiskSnapshot::new(
        RiskAccountId::new(1)?,
        session(2)?,
        &[row(1, [0, 120, 0, 1], 86400)?, row(2, [0; 4], 86400)?],
    )?;
    assert_eq!(
        state.reset(3, RiskOperatorId::new(7)?, &carried, at(86400)?),
        Err(RiskError::Reset)
    );
    let old_valuation = RiskSnapshot::new(
        RiskAccountId::new(1)?,
        session(2)?,
        &[row(1, [0; 4], 86389)?, row(2, [0; 4], 86400)?],
    )?;
    assert_eq!(
        state.reset(3, RiskOperatorId::new(7)?, &old_valuation, at(86400)?),
        Err(RiskError::Reset)
    );
    let healthy = RiskSnapshot::new(
        RiskAccountId::new(1)?,
        session(2)?,
        // A qualified prior-session mark can be within age; the loss baseline
        // still belongs to the new session and must be established by P08.
        &[row(1, [0, 20, 0, 1], 86399)?, row(2, [0; 4], 86400)?],
    )?;
    let _receipt = state.reset(3, RiskOperatorId::new(7)?, &healthy, at(86400)?)?;
    assert_eq!(
        state.observe(4, &healthy, at(86400)?)?,
        RiskDecision::Permitted
    );
    Ok(())
}

#[test]
fn arithmetic_and_counter_scopes_never_wrap_or_partially_mutate_state() -> TestResult {
    assert_eq!(
        RiskUsage::new(
            UsdCents::new(u64::MAX),
            UsdCents::new(1),
            UsdCents::new(0),
            0
        )
        .err(),
        Some(RiskError::Overflow)
    );
    for values in [
        ([u64::MAX, 0, 0, 0], [1, 0, 0, 0]),
        ([0, 0, u64::MAX, 0], [0, 0, 1, 0]),
        ([0, 0, 0, u64::MAX], [0, 0, 0, 1]),
    ] {
        let mut state = state(policy(
            caps(u64::MAX, u64::MAX, u64::MAX),
            caps(u64::MAX, u64::MAX, u64::MAX),
        )?)?;
        let snapshot = snapshot(row(1, values.0, 10)?, row(2, values.1, 10)?)?;
        assert_eq!(
            state.observe(1, &snapshot, at(10)?),
            Err(RiskError::Overflow)
        );
        assert_eq!(state.status(), (1, None));
    }
    let mut state = state(standard()?)?;
    assert_eq!(
        state.check_increase(
            1,
            &healthy()?,
            at(10)?,
            RiskIncrease::new(RiskStrategyId::new(1)?, UsdCents::new(u64::MAX), 0)
        ),
        Err(RiskError::Overflow)
    );
    assert_eq!(state.status(), (1, None));
    Ok(())
}

#[test]
fn policy_is_bounded_content_pinned_and_safe_to_diagnose() -> TestResult {
    assert_eq!(RiskAccountId::new(0).err(), Some(RiskError::Configuration));
    let operator = RiskOperatorId::new(7)?;
    let strategies = [
        (RiskStrategyId::new(1)?, caps(100, 20, 4)),
        (RiskStrategyId::new(2)?, caps(100, 20, 4)),
    ];
    for (age, rows, actors) in [
        (Duration::ZERO, strategies.as_slice(), vec![operator]),
        (Duration::from_secs(10), &[][..], vec![operator]),
        (Duration::from_secs(10), strategies.as_slice(), vec![]),
        (
            Duration::from_secs(10),
            strategies.as_slice(),
            vec![operator, operator],
        ),
        (
            Duration::from_secs(10),
            strategies.as_slice(),
            vec![operator; 17],
        ),
    ] {
        assert_eq!(
            RiskPolicy::new(rules()?, age, caps(100, 20, 4), rows, &actors).err(),
            Some(RiskError::Configuration)
        );
    }
    let standard = standard()?;
    // Independently encoded with Python hashlib and the documented v1 widths.
    assert_eq!(
        standard.fingerprint().bytes(),
        [
            0xeb, 0x16, 0x03, 0x26, 0xb2, 0x1f, 0xf1, 0x3e, 0x0e, 0x34, 0x28, 0xe9, 0x4b, 0x40,
            0x3d, 0x13, 0xd3, 0xb1, 0xee, 0xe3, 0xc6, 0xb2, 0xf0, 0x19, 0xce, 0xeb, 0x80, 0xfe,
            0x11, 0xc8, 0xe2, 0xab,
        ]
    );
    let reordered = RiskPolicy::new(
        rules()?,
        Duration::from_secs(10),
        caps(100, 20, 4),
        &[strategies[1], strategies[0]],
        &[operator],
    )?;
    assert_eq!(standard.fingerprint(), reordered.fingerprint());
    assert_ne!(
        standard.fingerprint(),
        policy(caps(101, 20, 4), caps(100, 20, 4))?.fingerprint()
    );
    let debug = format!(
        "{:?}{:?}{:?}",
        UsdCents::new(987_654_321),
        row(1, [987_654_321, 0, 0, 0], 10)?,
        healthy()?
    );
    assert!(!debug.contains("987654321"));
    assert_eq!(
        format!("{}", RiskError::Scope),
        "simulation risk rejected: Scope"
    );
    Ok(())
}

#[test]
fn maximum_complete_policy_is_supported_and_session_claims_cannot_overlap() -> TestResult {
    let strategies: Vec<_> = (1..=32)
        .map(|id| Ok((RiskStrategyId::new(id)?, caps(0, 0, 0))))
        .collect::<Result<_, RiskError>>()?;
    let operators = (1..=16)
        .map(RiskOperatorId::new)
        .collect::<Result<Vec<_>, _>>()?;
    let policy = RiskPolicy::new(
        rules()?,
        Duration::from_secs(1),
        caps(0, 0, 0),
        &strategies,
        &operators,
    )?;
    let mut owner = state(policy.clone())?;
    let rows = (1..=32)
        .map(|id| row(id, [0; 4], 10))
        .collect::<Value<Vec<_>>>()?;
    let full = RiskSnapshot::new(RiskAccountId::new(1)?, session(1)?, &rows)?;
    assert_eq!(owner.observe(1, &full, at(10)?)?, RiskDecision::Permitted);
    let operator = RiskOperatorId::new(1)?;
    assert_eq!(
        RiskPolicy::new(
            rules()?,
            Duration::from_secs(1),
            caps(0, 0, 0),
            &[strategies[0], strategies[0]],
            &[operator]
        )
        .err(),
        Some(RiskError::Configuration)
    );
    let mut oversized = strategies;
    oversized.push((RiskStrategyId::new(33)?, caps(0, 0, 0)));
    assert_eq!(
        RiskPolicy::new(
            rules()?,
            Duration::from_secs(1),
            caps(0, 0, 0),
            &oversized,
            &[operator]
        )
        .err(),
        Some(RiskError::Configuration)
    );
    assert_eq!(
        RiskState::new(RiskAccountId::new(1)?, policy, session(1)?, at(100)?).err(),
        Some(RiskError::Session)
    );
    let overlapping = RiskSession::new(
        SessionDate::new(1970, 1, 2)?,
        EvidenceInterval::new(at(50)?, at(200)?)?,
        rules()?,
    );
    assert_eq!(
        owner.rollover(2, operator, overlapping, at(150)?).err(),
        Some(RiskError::Session)
    );
    assert_eq!(owner.status(), (2, None));
    assert_eq!(
        owner.check_increase(
            2,
            &full,
            at(10)?,
            RiskIncrease::new(RiskStrategyId::new(1)?, UsdCents::new(1), 0)
        )?,
        RiskDecision::Denied(RiskReason::AccountGross)
    );
    portable::<RiskState>();
    portable::<RiskSnapshot>();
    Ok(())
}
