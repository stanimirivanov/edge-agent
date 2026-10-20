//! Real independently spawned workers and PostgreSQL. No mock transaction proof.
#[path = "accounts/fixture.rs"]
mod fixture;
use edgeagent_contracts::simulation::AdmissionStatus;
use fixture::{Database, Error};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;

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
