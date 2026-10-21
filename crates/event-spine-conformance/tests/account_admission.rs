//! Real independently spawned workers and PostgreSQL. No mock transaction proof.
#[path = "accounts/fixture.rs"]
mod fixture;
use edgeagent_contracts::simulation::AdmissionStatus;
use fixture::{Database, Error};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn audit_owner_export_rebuild_checkpoint_and_reordered_backfill_reconcile()
-> Result<(), Error> {
    use edgeagent_audit_domain::{AccountProjection, ReplayError, ReplayOutcome};
    let mut database = Database::new("execution-settlement-call-itm").await?;
    let result = AssertUnwindSafe(async {
        for index in 0..9 {
            database
                .spawn(index, "apply", 1, &format!("audit-{index}"))?
                .success()
                .await?;
        }
        database.assert_audit_grants().await?;
        assert!(database.audit_page(None, 2, 9).await.is_err());
        assert!(database.audit_page(None, 0, 1).await.is_err());
        assert!(database.audit_page(Some(10), 2, 1).await.is_err());
        let mut cursor = None;
        let mut frames = Vec::new();
        loop {
            let page = database.audit_page(cursor, 2, 1).await?;
            assert_eq!(page.owner_revision, 9);
            if page.frames.is_empty() {
                break;
            }
            cursor = page.frames.last().map(|f| f.state().revision);
            frames.extend(page.frames);
        }
        assert_eq!(frames.len(), 10);
        let genesis = frames.first().ok_or("genesis missing")?.clone();
        let mut view = AccountProjection::new(genesis.clone(), 10)?;
        let last = frames.last().ok_or("last missing")?.clone();
        assert_eq!(view.apply(last.clone()), Err(ReplayError::Gap));
        for frame in frames.iter().skip(1) {
            view.apply(frame.clone())?;
            // Independent continuation restore on every revision, not hidden process state.
            view = AccountProjection::restore(&genesis, &view.checkpoint()?)?;
        }
        let before = view.checkpoint()?;
        for frame in frames.iter().rev() {
            assert_eq!(view.apply(frame.clone())?, ReplayOutcome::Duplicate);
        }
        assert_eq!(view.checkpoint()?, before);
        let mut rebuild = AccountProjection::new(genesis, 10)?;
        for frame in frames.iter().skip(1) {
            rebuild.apply(frame.clone())?;
        }
        assert_eq!(rebuild.checkpoint()?, before);
        let actual = database.settlement_state().await?;
        assert_eq!(view.state().cash_cents, u64::try_from(actual.0)?);
        assert!(
            view.state()
                .positions
                .iter()
                .all(|p| p.quantity == 0 && p.cost_cents == Some(0))
        );
        assert!(
            view.state()
                .orders
                .iter()
                .all(|o| o.reserved_cash_cents == 0 && o.reserved_units == 0)
        );
        assert_eq!(view.state().revision, 9);
        assert_eq!(view.lag(9)?, 0);
        for frame in &frames {
            assert_eq!(
                frame
                    .state()
                    .postings
                    .iter()
                    .map(|n| i128::from(*n))
                    .sum::<i128>(),
                0
            );
            for value in &frame.state().valuations {
                let initial = frames[0]
                    .state()
                    .valuations
                    .iter()
                    .find(|v| v.strategy == value.strategy)
                    .ok_or("valuation missing")?;
                assert_eq!(
                    (value.valued_seconds, value.valued_nanos),
                    (initial.valued_seconds, initial.valued_nanos)
                );
            }
        }
        database.assert_settlement_journals().await?;
        Ok::<_, Error>(())
    })
    .catch_unwind()
    .await;
    database.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn audit_history_rolls_back_with_worker_death_and_lost_result_replays_once()
-> Result<(), Error> {
    let mut database = Database::new("cash").await?;
    let result = AssertUnwindSafe(async {
        database
            .spawn(0, "rollback", 1, "audit-rollback")?
            .failure("p08_injected_failure")
            .await?;
        assert!(database.audit_page(None, 8, 1).await.is_err());
        let mut killed = database.spawn(0, "before", 1, "audit-killed")?;
        killed.stage("READY").await?;
        killed.kill().await?;
        assert!(database.audit_page(None, 8, 1).await.is_err());
        let mut lost = database.spawn(0, "after", 1, "audit-lost")?;
        lost.stage("COMMITTED").await?;
        lost.kill().await?;
        let first = database.audit_page(None, 8, 1).await?;
        assert_eq!(first.frames.len(), 2);
        database
            .spawn(0, "apply", 1, "audit-semantic-retry")?
            .success()
            .await?;
        assert_eq!(database.audit_page(None, 8, 1).await?.frames, first.frames);
        let before = database.checkpoints().await?;
        database.remove_audit_history().await?;
        database
            .spawn(1, "apply", 1, "audit-missing-history")?
            .failure("audit_history_missing")
            .await?;
        assert_eq!(database.checkpoints().await?, before);
        database.restore_audit_history(&first.frames).await?;
        database
            .spawn(1, "apply", 1, "audit-denied-second")?
            .success()
            .await?;
        let page = database.audit_page(None, 8, 1).await?;
        assert_eq!(page.frames.len(), 3);
        assert_eq!(
            page.frames.last().ok_or("missing view")?.state().cash_cents,
            10_000
        );
        Ok::<_, Error>(())
    })
    .catch_unwind()
    .await;
    database.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn settlement_payoffs_defer_without_releasing_holds_and_close_once_across_strategies()
-> Result<(), Error> {
    for case in [
        "execution-settlement-call-itm",
        "execution-settlement-call-atm",
        "execution-settlement-call-otm",
        "execution-settlement-put-itm",
        "execution-settlement-put-atm",
        "execution-settlement-put-otm",
        "execution-settlement-fractional",
        "execution-settlement-empty",
    ] {
        let mut database = Database::new(case).await?;
        let result = AssertUnwindSafe(async {
            for index in 0..5 {
                database
                    .spawn(index, "apply", 1, &format!("initial-{index}"))?
                    .success()
                    .await?;
            }
            let empty = case.ends_with("empty");
            let before = if empty {
                (7000, 0, 0, 0, 0, 0)
            } else {
                (23999, 3, 18000, 18003, 0, 0)
            };
            assert_eq!(database.settlement_state().await?, before);
            for (index, status) in [(5, "deferred_missing"), (6, "deferred_not_known")] {
                database
                    .spawn(index, "apply", 1, &format!("defer-{index}"))?
                    .success()
                    .await?;
                let state = database.settlement_state().await?;
                assert_eq!(
                    (state.0, state.1, state.2, state.3, state.4),
                    (before.0, before.1, before.2, before.3, 0)
                );
                let fact = database.settlement_outcome(index as i64 + 1).await?;
                assert_eq!(fact.position().2, status);
                assert!(fact.effects().0.is_empty());
                assert!(fact.effects().1.is_empty());
            }
            database.spawn(7, "apply", 1, "settled")?.success().await?;
            let gross = if case.ends_with("-itm") {
                150000
            } else if case.ends_with("fractional") {
                2
            } else {
                0
            };
            let expected = if empty { 7000 } else { 23999 + gross - 3 };
            assert_eq!(
                database.settlement_state().await?,
                (expected, 0, 0, 0, 1, 3)
            );
            database.assert_settlement_journals().await?;
            let fact = database.settlement_outcome(8).await?;
            assert_eq!(fact.position().2, "settled");
            assert_eq!(fact.effects().1.len(), if empty { 0 } else { 2 });
            let mut totals = [0_i128; 4];
            let mut allocated_gross = 0;
            let mut allocated_fees = 0;
            for allocation in fact.effects().0 {
                let (_, _, g, f, p) = allocation.parts();
                allocated_gross += g;
                allocated_fees += f;
                assert_eq!(p.iter().map(|n| i128::from(*n)).sum::<i128>(), 0);
                for (total, n) in totals.iter_mut().zip(p) {
                    *total += i128::from(n);
                }
            }
            assert_eq!(
                (allocated_gross, allocated_fees),
                if empty { (0, 0) } else { (gross as u64, 3) }
            );
            assert_eq!(totals.iter().sum::<i128>(), 0);
            let checkpoint = database.checkpoints().await?;
            database
                .spawn(7, "apply", 1, "semantic-settlement-retry")?
                .success()
                .await?;
            assert_eq!(database.checkpoints().await?, checkpoint);
            assert_eq!(database.settlement_outcome(8).await?, fact);
            database
                .spawn(8, "apply", 1, "already-settled")?
                .success()
                .await?;
            assert_eq!(
                database.settlement_state().await?,
                (expected, 0, 0, 0, 1, 4)
            );
            assert_eq!(
                database.settlement_outcome(9).await?.position().2,
                "already_settled"
            );
            Ok::<_, Error>(())
        })
        .catch_unwind()
        .await;
        database.finish(result).await?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn settlement_worker_death_and_lost_result_preserve_atomic_journals_and_cash()
-> Result<(), Error> {
    let mut database = Database::new("execution-settlement-call-itm").await?;
    let result = AssertUnwindSafe(async {
        for index in 0..7 {
            database
                .spawn(index, "apply", 1, &format!("initial-{index}"))?
                .success()
                .await?;
        }
        let before = database.settlement_state().await?;
        let checkpoint = database.checkpoints().await?;
        database
            .spawn(7, "rollback", 1, "rolled-back-settlement")?
            .failure("p08_injected_failure")
            .await?;
        assert_eq!(database.settlement_state().await?, before);
        assert_eq!(database.checkpoints().await?, checkpoint);
        let mut killed = database.spawn(7, "before", 1, "killed-settlement")?;
        killed.stage("READY").await?;
        killed.kill().await?;
        assert_eq!(database.settlement_state().await?, before);
        assert_eq!(database.checkpoints().await?, checkpoint);
        let mut lost = database.spawn(7, "after", 1, "lost-settlement-result")?;
        lost.stage("COMMITTED").await?;
        lost.kill().await?;
        assert_eq!(database.settlement_state().await?, (173996, 0, 0, 0, 1, 3));
        let committed = database.checkpoints().await?;
        let fact = database.settlement_outcome(8).await?;
        for id in ["lost-settlement-result", "new-settlement-delivery"] {
            database.spawn(7, "apply", 1, id)?.success().await?;
        }
        assert_eq!(database.checkpoints().await?, committed);
        assert_eq!(database.settlement_outcome(8).await?, fact);
        database.assert_settlement_journals().await?;
        Ok::<_, Error>(())
    })
    .catch_unwind()
    .await;
    database.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn settlement_competitors_authorization_conflicts_and_tampering_cannot_repay_a_position()
-> Result<(), Error> {
    let mut unfunded = Database::new("execution-settlement-unfunded").await?;
    let result = AssertUnwindSafe(async {
        for index in 0..7 {
            unfunded
                .spawn(index, "apply", 1, &format!("initial-{index}"))?
                .success()
                .await?;
        }
        let before = unfunded.settlement_state().await?;
        let checkpoint = unfunded.checkpoints().await?;
        unfunded
            .spawn(7, "apply", 1, "unfunded-settlement")?
            .failure("settlement_accounting")
            .await?;
        assert_eq!(unfunded.settlement_state().await?, before);
        assert_eq!(unfunded.checkpoints().await?, checkpoint);
        Ok::<_, Error>(())
    })
    .catch_unwind()
    .await;
    unfunded.finish(result).await?;
    let mut database = Database::new("execution-settlement-call-itm").await?;
    let result = AssertUnwindSafe(async {
        for index in 0..7 {
            database
                .spawn(index, "apply", 1, &format!("initial-{index}"))?
                .success()
                .await?;
        }
        let before = database.settlement_state().await?;
        database
            .spawn(7, "apply", 2, "unauthorized-settlement")?
            .failure("settlement_unauthorized")
            .await?;
        database
            .spawn(8, "apply", 1, "settlement-gap")?
            .failure("settlement_input_gap")
            .await?;
        assert_eq!(database.settlement_state().await?, before);
        let mut first = database.spawn(7, "before", 1, "first-settlement")?;
        first.stage("READY").await?;
        let mut second = database.spawn(7, "apply", 1, "competing-settlement")?;
        database.wait_for_root_lock(&second.application).await?;
        first.release().await?;
        first.success().await?;
        second.success().await?;
        assert_eq!(database.settlement_state().await?, (173996, 0, 0, 0, 1, 3));
        let checkpoint = database.checkpoints().await?;
        database
            .spawn_conflict(7)?
            .failure("settlement_identity")
            .await?;
        database
            .spawn(3, "apply", 1, "old-fill-after-settlement")?
            .success()
            .await?;
        assert_eq!(database.settlement_state().await?, (173996, 0, 0, 0, 1, 3));
        assert_eq!(database.checkpoints().await?, checkpoint);
        database.tamper_settlement().await?;
        database
            .spawn(7, "apply", 1, "tampered-plan")?
            .failure("settlement_configuration")
            .await?;
        assert_eq!(database.settlement_state().await?, (173996, 0, 0, 0, 1, 3));
        Ok::<_, Error>(())
    })
    .catch_unwind()
    .await;
    database.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn execution_fill_accounting_and_shared_liquidity_hold_across_products() -> Result<(), Error>
{
    for case in [
        "execution",
        "execution-sell",
        "execution-options",
        "execution-full",
        "execution-denied",
    ] {
        let mut database = Database::new(case).await?;
        let result = AssertUnwindSafe(async {
            for index in 0..=8 {
                database
                    .spawn(index, "apply", 1, &format!("step-{index}"))?
                    .success()
                    .await?;
            }
            let expected = if case == "execution-denied" {
                (7_000, 0, 0, 0, 7, 0)
            } else if case == "execution-sell" {
                (35_999, 4, 20_000, 1, 7, 1)
            } else {
                (23_999, 1, 6_000, 1, 7, 1)
            };
            assert_eq!(database.execution_state().await?, expected);
            database.assert_counts(2, 0, 9, 9).await?;
            if matches!(case, "execution-full" | "execution-denied") {
                let fact = edgeagent_contracts::MessageEnvelope::from_json(
                    &database.execution_outcome(7).await?,
                )?;
                let decision: edgeagent_contracts::execution::ExecutionDecision = fact.payload()?;
                assert_eq!(decision.effect(), ("already_terminal", 0));
                assert_eq!(decision.postings(), [0; 4]);
                assert_eq!(
                    decision.position().2,
                    if case == "execution-full" { 4 } else { 1 }
                );
            }
            database
                .assert_ledger_and_valuation(if case == "execution-sell" { -1000 } else { 0 })
                .await?;
            let at_end = database.checkpoints().await?;
            let original = database.execution_outcome(4).await?;
            database
                .spawn(3, "apply", 1, "new-delivery-of-old-fill")?
                .success()
                .await?;
            assert_eq!(database.execution_outcome(4).await?, original);
            assert_eq!(database.checkpoints().await?, at_end);
            database
                .spawn_conflict(3)?
                .failure("execution_identity")
                .await?;
            database
                .tamper_execution_artifact(case == "execution")
                .await?;
            database
                .spawn(3, "apply", 1, "corrupt-retained-configuration")?
                .failure("execution_configuration")
                .await?;
            assert_eq!(database.execution_state().await?, expected);
            assert_eq!(database.checkpoints().await?, at_end);
            Ok::<_, Error>(())
        })
        .catch_unwind()
        .await;
        database.finish(result).await?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn execution_crash_rollback_and_lost_ack_retain_one_fill() -> Result<(), Error> {
    let mut database = Database::new("execution").await?;
    let result = AssertUnwindSafe(async {
        for index in 0..=2 {
            database
                .spawn(index, "apply", 1, &format!("initial-{index}"))?
                .success()
                .await?;
        }
        let before = database.execution_state().await?;
        let checkpoint = database.checkpoints().await?;
        database
            .spawn(3, "rollback", 1, "rolled-back-fill")?
            .failure("p08_injected_failure")
            .await?;
        assert_eq!(database.execution_state().await?, before);
        assert_eq!(database.checkpoints().await?, checkpoint);
        let mut killed = database.spawn(3, "before", 1, "killed-fill")?;
        killed.stage("READY").await?;
        killed.kill().await?;
        assert_eq!(database.execution_state().await?, before);
        let mut lost_ack = database.spawn(3, "after", 1, "committed-no-ack")?;
        lost_ack.stage("COMMITTED").await?;
        lost_ack.kill().await?;
        assert_eq!(
            database.execution_state().await?,
            (23_999, 1, 6_000, 1, 2, 1)
        );
        let committed = database.checkpoints().await?;
        let fact = database.execution_outcome(4).await?;
        database
            .spawn(3, "apply", 1, "committed-no-ack")?
            .success()
            .await?;
        database
            .spawn(3, "apply", 1, "semantic-fill-retry")?
            .success()
            .await?;
        assert_eq!(database.checkpoints().await?, committed);
        assert_eq!(database.execution_outcome(4).await?, fact);
        database
            .spawn(4, "apply", 1, "durable-pending-cancel")?
            .success()
            .await?;
        let pending = database.checkpoints().await?;
        let mut cancel = database.spawn(7, "before", 1, "early-missing-predecessor")?;
        cancel.failure("execution_input_gap").await?;
        assert_eq!(database.checkpoints().await?, pending);
        for index in 5..=8 {
            database
                .spawn(index, "apply", 1, &format!("restart-{index}"))?
                .success()
                .await?;
        }
        database.assert_reserved(0, 0).await?;
        database.assert_ledger_and_valuation(0).await?;
        Ok::<_, Error>(())
    })
    .catch_unwind()
    .await;
    database.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn execution_competing_workers_serialize_without_reusing_liquidity() -> Result<(), Error> {
    let mut database = Database::new("execution").await?;
    let result = AssertUnwindSafe(async {
        for index in 0..=2 {
            database
                .spawn(index, "apply", 1, &format!("initial-{index}"))?
                .success()
                .await?;
        }
        let mut first = database.spawn(3, "before", 1, "winner")?;
        first.stage("READY").await?;
        let mut second = database.spawn(3, "apply", 1, "competing-same-fill")?;
        database.wait_for_root_lock(&second.application).await?;
        first.release().await?;
        first.success().await?;
        second.success().await?;
        assert_eq!(
            database.execution_state().await?,
            (23_999, 1, 6_000, 1, 2, 1)
        );
        let snapshot = database.execution_state().await?;
        database
            .spawn(4, "apply", 2, "unauthorized-next")?
            .failure("execution_unauthorized")
            .await?;
        assert_eq!(database.execution_state().await?, snapshot);
        for index in 4..=6 {
            database
                .spawn(index, "apply", 1, &format!("competing-strategy-{index}"))?
                .success()
                .await?;
        }
        assert_eq!(
            database.execution_state().await?,
            (23_999, 1, 6_000, 1, 5, 1)
        );
        database.assert_reserved(18_003, 0).await?;
        Ok::<_, Error>(())
    })
    .catch_unwind()
    .await;
    database.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn competing_processes_cannot_overspend_cash_inventory_or_option_contracts()
-> Result<(), Error> {
    for case in ["cash", "inventory", "options"] {
        let mut database = Database::new(case).await?;
        let result = AssertUnwindSafe(async {
            let mut first = database.spawn(0, "before", 1, "first")?;
            first.stage("READY").await?;
            let mut second = database.spawn(1, "apply", 1, "second")?;
            database.wait_for_root_lock(&second.application).await?;
            first.release().await?;
            first.success().await?;
            second.success().await?;
            database.assert_counts(2, 1, 2, 2).await?;
            let expected = if case == "inventory" {
                AdmissionStatus::InsufficientInventory
            } else {
                AdmissionStatus::InsufficientCash
            };
            assert_eq!(database.outcome(1).await?.decision(), (2, expected));
            database
                .assert_reserved(
                    if case == "inventory" { 0 } else { 6000 },
                    if case == "inventory" { 4 } else { 0 },
                )
                .await?;
            Ok::<_, Error>(())
        })
        .catch_unwind()
        .await;
        database.finish(result).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn rollback_process_crash_restart_and_lost_ack_preserve_one_effect() -> Result<(), Error> {
    let mut database = Database::new("cash").await?;
    let result = AssertUnwindSafe(async {
        let original = database.checkpoints().await?;
        database
            .spawn(1, "apply", 1, "advance-account")?
            .failure("admission_input_gap")
            .await?;
        let mut failed = database.spawn(0, "rollback", 1, "failed")?;
        failed.failure("p08_injected_failure").await?;
        database.assert_counts(0, 0, 0, 0).await?;
        assert_eq!(database.checkpoints().await?, original);
        let mut crashed = database.spawn(0, "before", 1, "crashed")?;
        crashed.stage("READY").await?;
        crashed.kill().await?;
        database.assert_counts(0, 0, 0, 0).await?;
        assert_eq!(database.checkpoints().await?, original);
        let mut committed = database.spawn(0, "after", 1, "lost-ack")?;
        committed.stage("COMMITTED").await?;
        committed.kill().await?;
        database.assert_counts(1, 1, 1, 1).await?;
        let retained = database.outcome_bytes(0).await?;
        let mut redelivered = database.spawn(0, "apply", 1, "lost-ack")?;
        redelivered.success().await?;
        database.assert_counts(1, 1, 1, 1).await?;
        // A completed order still resolves after a different input advances the
        // root and consumes the remaining canonical slot (denied for cash here).
        database
            .spawn(1, "apply", 1, "advance-account")?
            .success()
            .await?;
        database.assert_counts(2, 1, 2, 2).await?;
        let checkpoint = database.checkpoints().await?;
        let mut semantic = database.spawn(0, "apply", 1, "new-transport-id")?;
        semantic.success().await?;
        database.assert_counts(2, 1, 2, 3).await?;
        assert_eq!(database.outcome_bytes(0).await?, retained);
        assert_eq!(database.checkpoints().await?, checkpoint);
        let mut conflict = database.spawn_conflict(0)?;
        conflict.failure("admission_identity").await?;
        database.assert_counts(2, 1, 2, 3).await?;
        Ok::<_, Error>(())
    })
    .catch_unwind()
    .await;
    database.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn aggregate_strategy_risk_and_breach_latches_survive_new_workers() -> Result<(), Error> {
    for case in ["account-risk", "strategy-risk", "valuation-stale"] {
        let mut database = Database::new(case).await?;
        let result = AssertUnwindSafe(async {
            database
                .spawn(0, "apply", 1, "risk-first")?
                .success()
                .await?;
            if case == "valuation-stale" {
                database.refresh_valuations().await?;
            }
            database
                .spawn(1, "apply", 1, "risk-second")?
                .success()
                .await?;
            let first = database.outcome(0).await?;
            let second = database.outcome(1).await?;
            let page = database.audit_page(None, 8, 1).await?;
            let latest = page.frames.last().ok_or("missing risk audit")?.state();
            assert_eq!(latest.revision, 2);
            assert_eq!(
                latest.risk_checkpoint,
                edgeagent_simulation_postgres::hex(&database.checkpoints().await?.2)
            );
            if case == "valuation-stale" {
                assert_eq!(latest.risk_breach.as_deref(), Some("stale_valuation"));
            }
            match case {
                "account-risk" => {
                    assert_eq!(first.decision().1, AdmissionStatus::Admitted);
                    assert_eq!(second.decision().1, AdmissionStatus::RiskDenied);
                }
                "strategy-risk" => {
                    assert_eq!(first.decision().1, AdmissionStatus::RiskDenied);
                    assert_eq!(second.decision().1, AdmissionStatus::Admitted);
                }
                _ => {
                    assert_eq!(first.decision().1, AdmissionStatus::RiskDenied);
                    assert_eq!(second.decision().1, AdmissionStatus::RiskDenied);
                }
            }
            database
                .assert_counts(2, if case == "valuation-stale" { 0 } else { 1 }, 2, 2)
                .await?;
            Ok::<_, Error>(())
        })
        .catch_unwind()
        .await;
        database.finish(result).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL"]
async fn unauthorized_gapped_stale_and_tampered_quotes_leave_no_financial_state()
-> Result<(), Error> {
    for case in ["cash", "stale"] {
        let mut database = Database::new(case).await?;
        let result = AssertUnwindSafe(async {
            let original = database.checkpoints().await?;
            database
                .spawn(0, "apply", 2, "unauthorized")?
                .failure("admission_unauthorized")
                .await?;
            database
                .spawn(1, "apply", 1, "gap")?
                .failure("admission_input_gap")
                .await?;
            if case == "stale" {
                database
                    .spawn(0, "apply", 1, "expired")?
                    .failure("admission_quote_stale")
                    .await?;
            } else {
                database.tamper_quote().await?;
                database
                    .spawn(0, "apply", 1, "tampered")?
                    .failure("admission_quote_identity")
                    .await?;
            }
            database.assert_counts(0, 0, 0, 0).await?;
            assert_eq!(database.checkpoints().await?, original);
            Ok::<_, Error>(())
        })
        .catch_unwind()
        .await;
        database.finish(result).await?;
    }
    Ok(())
}
