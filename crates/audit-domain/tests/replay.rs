//! Synthetic owner exports only; these prove projection semantics, not trading fidelity.
use edgeagent_audit_domain::{AccountProjection, ReplayError, ReplayOutcome, fingerprint};
use edgeagent_contracts::audit::*;
type Error = Box<dyn std::error::Error>;
fn state(revision: u64) -> AccountAuditState {
    AccountAuditState {
        account: "1".repeat(32),
        run: "2".repeat(64),
        configuration: "3".repeat(64),
        execution_plan: Some("4".repeat(64)),
        settlement_plan: None,
        revision,
        input_revision: revision,
        cash_cents: 1000,
        risk_policy: "5".repeat(64),
        risk_revision: revision + 1,
        risk_breach: None,
        risk_checkpoint: "00".repeat(158),
        orders: vec![],
        positions: vec![AuditPosition {
            strategy: "1".repeat(32),
            instrument: "2".repeat(32),
            unit: "shares".into(),
            quantity: 10,
            cost_cents: Some(100),
        }],
        valuations: vec![AuditValuation {
            strategy: "1".repeat(32),
            gross_cents: 100,
            loss_cents: 0,
            valued_seconds: 1,
            valued_nanos: 0,
            artifact: "6".repeat(64),
        }],
        postings: [0; 4],
    }
}
fn genesis() -> Result<AccountAuditFrame, AuditContractError> {
    AccountAuditFrame::new(state(0), None)
}
fn next(previous: &AccountAuditFrame) -> Result<AccountAuditFrame, Error> {
    let mut s = previous.state().clone();
    s.revision += 1;
    s.input_revision += 1;
    s.risk_revision += 1;
    Ok(AccountAuditFrame::new(s, Some(fingerprint(previous)?))?)
}
#[test]
fn complete_rebuild_checkpoint_resume_and_reordered_backfill_agree() -> Result<(), Error> {
    let g = genesis()?;
    let a = next(&g)?;
    let b = next(&a)?;
    let mut view = AccountProjection::new(g.clone(), 4)?;
    let before = view.checkpoint()?;
    assert_eq!(view.apply(b.clone()), Err(ReplayError::Gap));
    assert_eq!(view.checkpoint()?, before);
    assert_eq!(view.apply(a.clone())?, ReplayOutcome::Applied);
    let mut restored = AccountProjection::restore(&g, &view.checkpoint()?)?;
    restored.apply(b.clone())?;
    let mut rebuilt = AccountProjection::new(g, 4)?;
    rebuilt.apply(a)?;
    rebuilt.apply(b)?;
    assert_eq!(restored.checkpoint()?, rebuilt.checkpoint()?);
    assert_eq!(restored.lag(4)?, 2);
    assert_eq!(restored.lag(1), Err(ReplayError::Invalid));
    Ok(())
}
#[test]
fn duplicates_conflicts_and_foreign_scope_never_mutate_a_view() -> Result<(), Error> {
    let g = genesis()?;
    let a = next(&g)?;
    let mut view = AccountProjection::new(g.clone(), 3)?;
    view.apply(a.clone())?;
    let before = view.checkpoint()?;
    assert_eq!(view.apply(g.clone())?, ReplayOutcome::Duplicate);
    assert_eq!(view.apply(a.clone())?, ReplayOutcome::Duplicate);
    let mut s = a.state().clone();
    s.risk_revision += 1;
    assert_eq!(
        view.apply(AccountAuditFrame::new(s, Some(fingerprint(&g)?))?),
        Err(ReplayError::Conflict)
    );
    for field in [
        "account",
        "run",
        "configuration",
        "risk_policy",
        "execution_plan",
        "settlement_plan",
    ] {
        let mut value = serde_json::to_value(&next(&a)?)?;
        let width = if field == "account" { 32 } else { 64 };
        value["state"][field] = serde_json::json!("a".repeat(width));
        let foreign = AccountAuditFrame::decode(&serde_json::to_vec(&value)?)?;
        assert_eq!(view.apply(foreign), Err(ReplayError::Scope));
    }
    let b = next(&a)?;
    let wrong = AccountAuditFrame::new(b.state().clone(), Some("0".repeat(64)))?;
    assert_eq!(view.apply(wrong), Err(ReplayError::Conflict));
    assert_eq!(view.checkpoint()?, before);
    let mut foreign = state(0);
    foreign.account = "b".repeat(32);
    let foreign = AccountAuditFrame::new(foreign, None)?;
    assert!(matches!(
        AccountProjection::restore(&foreign, &before),
        Err(ReplayError::Scope)
    ));
    let mut funding = state(0);
    funding.cash_cents += 1;
    let funding = AccountAuditFrame::new(funding, None)?;
    assert!(matches!(
        AccountProjection::restore(&funding, &before),
        Err(ReplayError::Conflict)
    ));
    Ok(())
}
#[test]
fn financial_drift_and_revision_regression_fail_atomically() -> Result<(), Error> {
    let g = genesis()?;
    let a = next(&g)?;
    let mut view = AccountProjection::new(g, 4)?;
    view.apply(a.clone())?;
    let before = view.checkpoint()?;
    for mutation in 0..4 {
        let mut s = state(2);
        match mutation {
            0 => s.cash_cents += 1,
            1 => s.positions[0].cost_cents = Some(101),
            2 => s.risk_revision = 1,
            _ => s.positions[0].cost_cents = None,
        }
        let frame = AccountAuditFrame::new(s, Some(fingerprint(&a)?))?;
        assert_eq!(view.apply(frame), Err(ReplayError::Invalid));
        assert_eq!(view.checkpoint()?, before);
    }
    let mut s = state(2);
    s.cash_cents = 949;
    s.positions[0].cost_cents = Some(150);
    s.postings = [-51, 50, 0, 1];
    view.apply(AccountAuditFrame::new(s, Some(fingerprint(&a)?))?)?;
    assert_eq!(view.state().postings, [-51, 50, 0, 1]);
    Ok(())
}
#[test]
fn capacity_corruption_truncation_and_unknown_versions_fail_closed() -> Result<(), Error> {
    let g = genesis()?;
    let a = next(&g)?;
    let mut view = AccountProjection::new(g.clone(), 1)?;
    assert_eq!(view.apply(a), Err(ReplayError::Capacity));
    let bytes = view.checkpoint()?;
    for index in [0, bytes.len() / 2, bytes.len() - 1] {
        let mut changed = bytes.clone();
        changed[index] ^= 1;
        assert!(AccountProjection::restore(&g, &changed).is_err());
        assert!(AccountProjection::restore(&g, &bytes[..index]).is_err());
    }
    assert!(AccountProjection::restore(&g, &vec![0; 3 * MAX_AUDIT_BYTES + 1]).is_err());
    assert!(AccountAuditFrame::decode(&vec![0; MAX_AUDIT_BYTES + 1]).is_err());
    let mut value = serde_json::to_value(genesis()?)?;
    value["version"] = "future-version".into();
    assert!(AccountAuditFrame::decode(&serde_json::to_vec(&value)?).is_err());
    Ok(())
}
#[test]
fn invalid_wire_state_ordering_reservations_and_nullable_presence_are_rejected() -> Result<(), Error>
{
    let mut s = state(1);
    let order = AuditOrder {
        order: "7".repeat(64),
        strategy: "1".repeat(32),
        instrument: "2".repeat(32),
        unit: "shares".into(),
        side: "buy_to_open".into(),
        revision: 1,
        status: "cancel_pending".into(),
        quantity: 10,
        filled: 0,
        reserved_cash_cents: 500,
        reserved_units: 0,
    };
    s.orders.push(order.clone());
    let good = AccountAuditFrame::new(s.clone(), Some("8".repeat(64)))?;
    assert_eq!(AccountAuditFrame::decode(&good.encode()?)?, good);
    for field in ["execution_plan", "settlement_plan"] {
        let mut value = serde_json::to_value(&good)?;
        value["state"]
            .as_object_mut()
            .ok_or("object")?
            .remove(field);
        assert!(AccountAuditFrame::decode(&serde_json::to_vec(&value)?).is_err());
    }
    for mutation in 0..6 {
        let mut s = s.clone();
        match mutation {
            0 => s.orders.push(order.clone()),
            1 => s.orders[0].reserved_cash_cents = 1001,
            2 => s.orders[0].status = "cancelled".into(),
            3 => s.input_revision = 2,
            4 => s.valuations[0].valued_nanos = 1_000_000_000,
            _ => s.positions.push(s.positions[0].clone()),
        }
        assert!(AccountAuditFrame::new(s, Some("8".repeat(64))).is_err());
    }
    Ok(())
}
#[test]
fn immutable_export_identity_and_payload_safe_diagnostics_are_stable() -> Result<(), Error> {
    let frame = genesis()?;
    let frozen = AccountAuditFrame::decode(include_bytes!("fixtures/genesis-v1.json"))?;
    assert_eq!(frame, frozen);
    let encoded = frame.encode()?;
    let text = String::from_utf8(encoded.clone())?;
    assert!(
        text.starts_with(
            "{\"version\":\"edgeagent.account-audit.v1\",\"previous\":null,\"state\":{"
        )
    );
    let digest = fingerprint(&frame)?;
    assert_eq!(digest, fingerprint(&AccountAuditFrame::decode(&encoded)?)?);
    assert!(!format!("{frame:?}").contains(&frame.state().account));
    assert!(!format!("{frame:?}").contains("cash_cents"));
    assert!(!AuditContractError.to_string().contains("1000"));
    Ok(())
}
