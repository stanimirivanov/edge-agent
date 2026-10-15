//! Real concurrent connections and controlled cancellation; no sleep-based races.

#![allow(
    clippy::disallowed_methods,
    reason = "Proofs administer owned schemas, observe locks and inject persisted state."
)]

#[path = "recovery/cancellation.rs"]
mod cancellation;
#[path = "recovery/concurrency.rs"]
mod concurrency;
#[path = "recovery/fixture.rs"]
mod fixture;
mod support;

use edgeagent_contracts::{Component, MessageDefinition, MessageEnvelope, MessageMetadata};
use edgeagent_messaging::{
    ClaimedMessage, FailureCode, LeaseDuration, OutboxRelayStore, OutboxRetryDelay,
};
use edgeagent_outbox_postgres::{
    EnqueueDisposition, OutboxErrorKind, PostgresOutbox, PostgresOutboxRelay, ReplayDisposition,
    ReplayRequest,
};
use serde_json::json;
use sqlx::{Connection, PgConnection};
use std::collections::BTreeSet;
use std::time::Duration;
use support::{Fixture, TestResult, with_fixture};
use tokio::time::timeout;

const OWNER: &str = "recovery_worker";
const FAILURE: FailureCode = FailureCode::from_static("transport_unavailable");
const SOURCE: &str = Component::Gateway.source_uri();
const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

fn envelope(id: &str, quantity: u16) -> TestResult<MessageEnvelope> {
    Ok(COMMAND.build(
        MessageMetadata {
            id: id.to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: COMMAND.message_type().to_owned(),
            subject: format!("order/{id}"),
            time: "2026-10-15T00:00:00Z".to_owned(),
            data_schema: COMMAND.data_schema().to_owned(),
            correlation_id: format!("correlation-{id}"),
            causation_id: format!("request-{id}"),
            idempotency_key: id.to_owned(),
            partition_key: format!("order/{id}"),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        },
        &json!({"quantity": quantity}),
    )?)
}

async fn seed(connection: &mut PgConnection, id: &str) -> TestResult {
    let mut transaction = connection.begin().await?;
    assert_eq!(
        PostgresOutbox
            .enqueue(&mut transaction, COMMAND, &envelope(id, 1)?)
            .await?,
        EnqueueDisposition::Inserted
    );
    transaction.commit().await?;
    Ok(())
}

async fn claim(connection: &mut PgConnection) -> TestResult<ClaimedMessage> {
    let mut transaction = connection.begin().await?;
    let mut claims = PostgresOutbox
        .claim_batch(
            &mut transaction,
            OWNER,
            1,
            LeaseDuration::new(LeaseDuration::MAX)?,
        )
        .await?;
    transaction.commit().await?;
    claims
        .pop()
        .ok_or_else(|| "expected one available claim".into())
}

fn replay_request(id: &str) -> TestResult<ReplayRequest<'_>> {
    Ok(ReplayRequest::new(
        id,
        "urn:edgeagent:operator:synthetic",
        "configuration_remediated",
    )?)
}

async fn backend_pid(connection: &mut PgConnection) -> TestResult<i32> {
    Ok(sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(connection)
        .await?)
}

async fn observe_blocked(observer: &mut PgConnection, waiter: i32, blocker: i32) -> TestResult {
    timeout(Duration::from_secs(2), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT $1 = ANY(pg_blocking_pids($2))")
                .bind(blocker)
                .bind(waiter)
                .fetch_one(&mut *observer)
                .await?;
            if blocked {
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}
