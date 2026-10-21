//! Real independently spawned workers and PostgreSQL. No mock transaction proof.
#[path = "accounts/fixture.rs"]
mod fixture;
use edgeagent_contracts::simulation::AdmissionStatus;
use fixture::{Database, Error};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;

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
