//! Real offline-worker death and wire-loss qualification, never vendor dispatch.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned schema administration and protocol fault fixtures."
)]
#[allow(
    dead_code,
    reason = "Reuse the owned M02 broker without duplicating its lifecycle"
)]
#[path = "../../../crates/event-spine-conformance/tests/topology/fixture.rs"]
mod broker;
#[path = "recovery/fixture.rs"]
mod fixture;
#[path = "recovery/postgres.rs"]
mod postgres;
#[path = "../../../crates/event-spine-conformance/tests/process/failures/proxy.rs"]
mod proxy;
#[path = "recovery/quota.rs"]
mod quota;
use edgeagent_market_data_application::*;
use fixture::*;
use std::time::Duration;
type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult = Result<(), TestError>;

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn killed_worker_before_terminal_commit_recovers_one_terminal_transition() -> TestResult {
    with_case(|h| {
        Box::pin(async move {
            let command = h.seed(1).await?;
            h.publish(&command, "before-commit").await?;
            let mut gate = postgres::CommitGate::start(&h.pg).await?;
            let worker = h.spawn(&["offline-worker"], Some(&gate.url), None)?;
            tokio::select! {
                result = gate.reached() => result?,
                exit = h.finish(worker) => return Err(format!("worker exited before COMMIT barrier: {:?}", exit?).into()),
            } // INSERT has executed, but its COMMIT has NOT reached PostgreSQL.
            assert_eq!(h.counts().await?, (0, 0, 0));
            assert_eq!(h.objects().await?, 1);
            assert_eq!(h.inventory().await?, 1);
            h.kill(worker).await?;
            gate.close().await?; // Close every upstream connection so PostgreSQL rolls back.
            assert_eq!(h.counts().await?, (0, 0, 0));
            let recovered = h.run(&["offline-worker"]).await?;
            assert!(recovered.contains("handled=1"));
            assert_eq!(h.counts().await?, (1, 1, 1));
            let state = h.ledger().load(&command).await?.1;
            assert_eq!(state.attempts, 2);
            assert_eq!((state.calls, state.charged_bytes), (0, 0));
            assert_eq!(
                h.objects().await?,
                2,
                "only uncommitted preparation repeats; both writes remain inventoried"
            );
            assert_eq!(h.inventory().await?, 2);
            assert_eq!(h.durable().await?.delivered.consumer_sequence, 2);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn ack_loss_and_same_store_broker_restart_preserve_completed_receipts() -> TestResult {
    for loss in [proxy::Loss::Request, proxy::Loss::Confirmation] {
        with_case(|h| {
            Box::pin(async move {
                let command = h.seed(1).await?;
                let delivery = h.publish(&command, "wire-loss").await?;
                let wire = proxy::Proxy::start(&h.broker_url().await, loss).await?;
                let worker_url = wire.url.clone();
                let worker = h.spawn(&["offline-worker"], None, Some(&worker_url))?;
                let evidence = tokio::select! {
                    result = wire.observed() => result?,
                    exit = h.finish(worker) => return Err(format!("worker exited before ACK barrier: {:?}", exit?).into()),
                };
                assert_eq!(evidence.requests, 1);
                // Check atomic state before the recovery API can insert an inbox marker.
                assert_eq!(h.counts().await?, (1, 1, 1));
                let receipt = h
                    .ledger()
                    .recover(&delivery)
                    .await?
                    .ok_or("receipt missing at ACK boundary")?;
                match loss {
                    proxy::Loss::Request => {
                        assert_eq!((evidence.forwarded, evidence.confirmations_dropped), (0, 0));
                        assert_eq!(h.durable().await?.num_ack_pending, 1);
                        h.kill(worker).await?; // Actual process death after commit, before broker ACK.
                        let durable_before = h.durable().await?;
                        assert_eq!(wire.finish().await?, evidence);
                        h.restart_broker().await?;
                        let retained = h.durable().await?;
                        assert_eq!(retained.created, durable_before.created);
                        assert_eq!(retained.num_ack_pending, 1);
                        let output = h.run(&["offline-worker"]).await?;
                        assert!(output.contains("retained_receipt=1"));
                    }
                    proxy::Loss::Confirmation => {
                        assert_eq!((evidence.forwarded, evidence.confirmations_dropped), (1, 1));
                        assert_eq!(h.durable().await?.num_ack_pending, 0);
                        let (status, _) = h.finish(worker).await?;
                        assert!(
                            !status.success(),
                            "worker must report unknown confirmation, not success"
                        );
                        assert_eq!(wire.finish().await?, evidence);
                        h.publish(&command, "logical-duplicate").await?;
                        assert!(
                            h.run(&["offline-worker"])
                                .await?
                                .contains("retained_receipt=1")
                        );
                    }
                }
                let info = h.durable().await?;
                assert_eq!(info.num_ack_pending, 0);
                assert_eq!(info.delivered.consumer_sequence, 2);
                let expected = if matches!(loss, proxy::Loss::Request) { (1, 1, 1) } else { (1, 2, 1) };
                assert_eq!(h.counts().await?, expected);
                assert_eq!(h.objects().await?, 1);
                assert_eq!(
                    h.ledger()
                        .recover(&delivery)
                        .await?
                        .ok_or("receipt")?
                        .bytes(),
                    receipt.bytes()
                );
                let state = h.ledger().load(&command).await?.1;
                assert_eq!(
                    (state.attempts, state.calls, state.charged_bytes),
                    (1, 0, 0)
                );
                Ok(())
            })
        })
        .await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn quota_process_death_preserves_shared_account_fences_and_offline_isolation() -> TestResult {
    with_case(|h| Box::pin(quota::prove(h))).await
}

// This is a libtest subprocess driver, not a recovery proof. Default execution
// checks its closed protocol and performs no I/O, even with ambient credentials.
#[test]
fn quota_child_protocol_and_process_entry() -> TestResult {
    quota::child_entry()
}

#[test]
fn fixture_endpoints_reject_remote_and_routing_overrides_before_io() -> TestResult {
    postgres_url("postgresql://fixture:local@127.0.0.1:5432/fixture")?;
    for url in [
        "postgresql://remote.invalid/fixture",
        "postgresql://127.0.0.1/fixture?host=remote.invalid",
        "postgresql://localhost/fixture#override",
        "https://localhost/fixture",
    ] {
        assert!(postgres_url(url).is_err());
    }
    Ok(())
}
