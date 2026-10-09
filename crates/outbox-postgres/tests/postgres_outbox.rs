//! Opt-in conformance test against the checked-in local PostgreSQL profile.

use edgeagent_contracts::{
    Component, EventRetention, MessageDefinition, MessageMetadata, MessageRegistry,
};
use edgeagent_messaging::{FailureCode, LeaseDuration, OutboxRetryDelay};
use edgeagent_outbox_postgres::{
    EnqueueDisposition, OutboxErrorKind, PostgresOutbox, ReplayDisposition, ReplayRequest,
};
use serde_json::json;
use sqlx::{Connection, PgConnection, Row};
use std::env;
use std::error::Error;
use std::process;
use std::time::Duration;

const TRANSPORT_UNAVAILABLE: FailureCode = FailureCode::from_static("transport_unavailable");
const TRANSPORT_REJECTED: FailureCode = FailureCode::from_static("transport_rejected");

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);
const EVENT: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.execution.dry-run-order-accepted.v1",
    "urn:edgeagent:schema:dry-run-order-accepted:v1",
    Component::ExecutionSimulator,
    "order",
    EventRetention::Audit,
);

fn metadata(source: Component) -> MessageMetadata {
    MessageMetadata {
        id: "outbox-message-01".to_owned(),
        source: source.source_uri().to_owned(),
        message_type: COMMAND.message_type().to_owned(),
        subject: "order/outbox-order-01".to_owned(),
        time: "2026-09-26T00:00:00Z".to_owned(),
        data_schema: COMMAND.data_schema().to_owned(),
        correlation_id: "outbox-correlation-01".to_owned(),
        causation_id: "outbox-request-01".to_owned(),
        idempotency_key: "outbox-order-01".to_owned(),
        partition_key: "order/outbox-order-01".to_owned(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        trace_state: None,
    }
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn transaction_identity_and_lease_invariants_hold() -> Result<(), Box<dyn Error>> {
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let mut client = PgConnection::connect(&postgres_url).await?;
    let schema = format!("edgeagent_outbox_test_{}", process::id());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; SET search_path TO {schema};"
    )))
    .execute(&mut client)
    .await?;
    for migration in PostgresOutbox::MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut client).await?;
    }

    let outbox = PostgresOutbox;
    let lease_duration = LeaseDuration::new(Duration::from_secs(30))?;
    let gateway_envelope = COMMAND.build(metadata(Component::Gateway), &json!({"quantity": 1}))?;
    let research_envelope =
        COMMAND.build(metadata(Component::Research), &json!({"quantity": 1}))?;

    let mut transaction = client.begin().await?;
    assert_eq!(
        outbox
            .enqueue(&mut transaction, COMMAND, &gateway_envelope)
            .await?,
        EnqueueDisposition::Inserted
    );
    transaction.rollback().await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_outbox")
        .fetch_one(&mut client)
        .await?;
    assert_eq!(count, 0);

    let mut transaction = client.begin().await?;
    assert_eq!(
        outbox
            .enqueue(&mut transaction, COMMAND, &gateway_envelope)
            .await?,
        EnqueueDisposition::Inserted
    );
    assert_eq!(
        outbox
            .enqueue(&mut transaction, COMMAND, &gateway_envelope)
            .await?,
        EnqueueDisposition::AlreadyPresent
    );
    assert_eq!(
        outbox
            .enqueue(&mut transaction, COMMAND, &research_envelope)
            .await?,
        EnqueueDisposition::Inserted
    );
    transaction.commit().await?;

    let conflicting = COMMAND.build(metadata(Component::Gateway), &json!({"quantity": 2}))?;
    let mut transaction = client.begin().await?;
    let conflict = outbox
        .enqueue(&mut transaction, COMMAND, &conflicting)
        .await;
    assert_eq!(
        conflict.err().map(|error| error.kind()),
        Some(OutboxErrorKind::MessageIdentityConflict)
    );
    let stored_bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT envelope FROM edgeagent_message_outbox WHERE message_source = $1 AND message_id = $2",
    )
    .bind(gateway_envelope.source())
    .bind(gateway_envelope.id())
    .fetch_one(&mut *transaction)
    .await?;
    assert_eq!(stored_bytes, gateway_envelope.to_json()?);
    transaction.rollback().await?;

    let mut transaction = client.begin().await?;
    let claimed = outbox
        .claim_batch(&mut transaction, "relay_01", 10, lease_duration)
        .await?;
    assert_eq!(claimed.len(), 2);
    let definitions = [COMMAND, EVENT];
    let registry = MessageRegistry::new(&definitions)?;
    for message in &claimed {
        assert_eq!(message.attempt(), 1);
        assert_eq!(
            message.validated_envelope(&registry)?.to_json()?,
            message.envelope_bytes()
        );
    }
    transaction.commit().await?;

    let first = &claimed[0];
    let second = &claimed[1];
    let mut transaction = client.begin().await?;
    let lost = outbox
        .mark_published(&mut transaction, first, "relay_02")
        .await;
    assert_eq!(
        lost.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    transaction.rollback().await?;

    let mut transaction = client.begin().await?;
    outbox
        .release_for_retry(
            &mut transaction,
            first,
            "relay_01",
            OutboxRetryDelay::IMMEDIATE,
            TRANSPORT_UNAVAILABLE,
        )
        .await?;
    outbox
        .mark_published(&mut transaction, second, "relay_01")
        .await?;
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    let retried = outbox
        .claim_batch(&mut transaction, "relay_02", 10, lease_duration)
        .await?;
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].attempt(), 2);
    outbox
        .quarantine(
            &mut transaction,
            &retried[0],
            "relay_02",
            TRANSPORT_REJECTED,
        )
        .await?;
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    assert!(
        outbox
            .claim_batch(&mut transaction, "relay_03", 10, lease_duration)
            .await?
            .is_empty()
    );
    transaction.commit().await?;

    let replay_request = ReplayRequest::new(
        "replay-request-01",
        "urn:edgeagent:operator:alice",
        "configuration_remediated",
    )?;
    let mut transaction = client.begin().await?;
    assert_eq!(
        outbox
            .replay_quarantined(
                &mut transaction,
                retried[0].message_source(),
                retried[0].message_id(),
                replay_request,
            )
            .await?,
        ReplayDisposition::Released
    );
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    assert_eq!(
        outbox
            .replay_quarantined(
                &mut transaction,
                retried[0].message_source(),
                retried[0].message_id(),
                replay_request,
            )
            .await?,
        ReplayDisposition::AlreadyRequested
    );
    let conflicting_request = ReplayRequest::new(
        "replay-request-01",
        "urn:edgeagent:operator:bob",
        "configuration_remediated",
    )?;
    let conflict = outbox
        .replay_quarantined(
            &mut transaction,
            retried[0].message_source(),
            retried[0].message_id(),
            conflicting_request,
        )
        .await;
    assert_eq!(
        conflict.err().map(|error| error.kind()),
        Some(OutboxErrorKind::ReplayRequestConflict)
    );
    let not_quarantined = outbox
        .replay_quarantined(
            &mut transaction,
            second.message_source(),
            second.message_id(),
            ReplayRequest::new(
                "replay-request-02",
                "urn:edgeagent:operator:alice",
                "configuration_remediated",
            )?,
        )
        .await;
    assert_eq!(
        not_quarantined.err().map(|error| error.kind()),
        Some(OutboxErrorKind::NotQuarantined)
    );
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    let replayed = outbox
        .claim_batch(&mut transaction, "relay_03", 10, lease_duration)
        .await?;
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].attempt(), 1);
    assert_eq!(replayed[0].envelope_bytes(), retried[0].envelope_bytes());
    outbox
        .quarantine(
            &mut transaction,
            &replayed[0],
            "relay_03",
            TRANSPORT_REJECTED,
        )
        .await?;
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    assert_eq!(
        outbox
            .replay_quarantined(
                &mut transaction,
                replayed[0].message_source(),
                replayed[0].message_id(),
                replay_request,
            )
            .await?,
        ReplayDisposition::AlreadyRequested
    );
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    assert!(
        outbox
            .claim_batch(&mut transaction, "relay_04", 10, lease_duration)
            .await?
            .is_empty()
    );
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    assert_eq!(
        outbox
            .replay_quarantined(
                &mut transaction,
                replayed[0].message_source(),
                replayed[0].message_id(),
                ReplayRequest::new(
                    "replay-request-03",
                    "urn:edgeagent:operator:alice",
                    "configuration_remediated",
                )?,
            )
            .await?,
        ReplayDisposition::Released
    );
    transaction.commit().await?;

    let mut transaction = client.begin().await?;
    let replayed_again = outbox
        .claim_batch(&mut transaction, "relay_04", 10, lease_duration)
        .await?;
    assert_eq!(replayed_again.len(), 1);
    assert_eq!(replayed_again[0].attempt(), 1);
    assert_eq!(
        replayed_again[0].envelope_bytes(),
        retried[0].envelope_bytes()
    );
    outbox
        .mark_published(&mut transaction, &replayed_again[0], "relay_04")
        .await?;
    transaction.commit().await?;

    let audit = sqlx::query(
        "SELECT requested_by, reason, prior_quarantine_reason, prior_attempt_count FROM edgeagent_message_outbox_replay_audit WHERE replay_request_id = $1",
    )
    .bind("replay-request-01")
    .fetch_one(&mut client)
    .await?;
    assert_eq!(
        audit.try_get::<String, _>("requested_by")?,
        "urn:edgeagent:operator:alice"
    );
    assert_eq!(
        audit.try_get::<String, _>("reason")?,
        "configuration_remediated"
    );
    assert_eq!(
        audit.try_get::<String, _>("prior_quarantine_reason")?,
        "transport_rejected"
    );
    assert_eq!(audit.try_get::<i32, _>("prior_attempt_count")?, 2);
    let audit_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_outbox_replay_audit")
            .fetch_one(&mut client)
            .await?;
    assert_eq!(audit_count, 2);

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
    )))
    .execute(&mut client)
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn same_owner_reclaim_rejects_stale_transitions_and_preserves_replay_fence()
-> Result<(), Box<dyn Error>> {
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let mut client = PgConnection::connect(&postgres_url).await?;
    let schema = format!("edgeagent_outbox_fence_test_{}", process::id());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; SET search_path TO {schema};"
    )))
    .execute(&mut client)
    .await?;
    for migration in PostgresOutbox::MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut client).await?;
    }

    let outbox = PostgresOutbox;
    let envelope = COMMAND.build(metadata(Component::Gateway), &json!({"quantity": 1}))?;
    let mut transaction = client.begin().await?;
    assert_eq!(
        outbox.enqueue(&mut transaction, COMMAND, &envelope).await?,
        EnqueueDisposition::Inserted
    );
    transaction.commit().await?;

    let owner = "relay_same_owner";
    let lease_duration = LeaseDuration::new(LeaseDuration::MAX)?;
    let mut transaction = client.begin().await?;
    let mut first_claims = outbox
        .claim_batch(&mut transaction, owner, 1, lease_duration)
        .await?;
    assert_eq!(first_claims.len(), 1);
    let first_claim = first_claims.remove(0);
    transaction.commit().await?;
    assert_eq!(first_claim.attempt(), 1);

    // Expire only the first claim without sleeping or depending on scheduler timing.
    let first_generation = i64::try_from(first_claim.lease_generation().get())?;
    let expired = sqlx::query(
        "UPDATE edgeagent_message_outbox SET lease_expires_at = clock_timestamp() - INTERVAL '1 millisecond' WHERE message_source = $1 AND message_id = $2 AND lease_owner = $3 AND lease_generation = $4",
    )
    .bind(first_claim.message_source())
    .bind(first_claim.message_id())
    .bind(owner)
    .bind(first_generation)
    .execute(&mut client)
    .await?;
    assert_eq!(expired.rows_affected(), 1);

    let mut transaction = client.begin().await?;
    let mut current_claims = outbox
        .claim_batch(&mut transaction, owner, 1, lease_duration)
        .await?;
    assert_eq!(current_claims.len(), 1);
    let current_claim = current_claims.remove(0);
    transaction.commit().await?;
    assert_eq!(current_claim.attempt(), 2);
    assert!(current_claim.lease_generation().get() > first_claim.lease_generation().get());

    let mut transaction = client.begin().await?;
    let stale_publish = outbox
        .mark_published(&mut transaction, &first_claim, owner)
        .await;
    assert_eq!(
        stale_publish.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    let stale_retry = outbox
        .release_for_retry(
            &mut transaction,
            &first_claim,
            owner,
            OutboxRetryDelay::IMMEDIATE,
            TRANSPORT_UNAVAILABLE,
        )
        .await;
    assert_eq!(
        stale_retry.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    let stale_quarantine = outbox
        .quarantine(&mut transaction, &first_claim, owner, TRANSPORT_REJECTED)
        .await;
    assert_eq!(
        stale_quarantine.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    transaction.commit().await?;

    let state = sqlx::query(
        "SELECT lease_owner, lease_generation, attempt_count, published_at IS NULL AS unpublished, quarantined_at IS NULL AS not_quarantined, last_failure_code, lease_expires_at > clock_timestamp() AS lease_current FROM edgeagent_message_outbox WHERE message_source = $1 AND message_id = $2",
    )
    .bind(current_claim.message_source())
    .bind(current_claim.message_id())
    .fetch_one(&mut client)
    .await?;
    assert_eq!(
        state
            .try_get::<Option<String>, _>("lease_owner")?
            .as_deref(),
        Some(owner)
    );
    assert_eq!(
        state.try_get::<i64, _>("lease_generation")?,
        i64::try_from(current_claim.lease_generation().get())?
    );
    assert_eq!(state.try_get::<i32, _>("attempt_count")?, 2);
    assert!(state.try_get::<bool, _>("unpublished")?);
    assert!(state.try_get::<bool, _>("not_quarantined")?);
    assert_eq!(
        state.try_get::<Option<String>, _>("last_failure_code")?,
        None
    );
    assert!(state.try_get::<bool, _>("lease_current")?);

    let mut transaction = client.begin().await?;
    outbox
        .quarantine(&mut transaction, &current_claim, owner, TRANSPORT_REJECTED)
        .await?;
    transaction.commit().await?;

    let replay_request = ReplayRequest::new(
        "fence-replay-01",
        "urn:edgeagent:operator:alice",
        "configuration_remediated",
    )?;
    let mut transaction = client.begin().await?;
    assert_eq!(
        outbox
            .replay_quarantined(
                &mut transaction,
                current_claim.message_source(),
                current_claim.message_id(),
                replay_request,
            )
            .await?,
        ReplayDisposition::Released
    );
    transaction.commit().await?;

    let released = sqlx::query(
        "SELECT lease_generation, attempt_count, quarantined_at IS NULL AS not_quarantined FROM edgeagent_message_outbox WHERE message_source = $1 AND message_id = $2",
    )
    .bind(current_claim.message_source())
    .bind(current_claim.message_id())
    .fetch_one(&mut client)
    .await?;
    assert_eq!(
        released.try_get::<i64, _>("lease_generation")?,
        i64::try_from(current_claim.lease_generation().get())?
    );
    assert_eq!(released.try_get::<i32, _>("attempt_count")?, 0);
    assert!(released.try_get::<bool, _>("not_quarantined")?);

    let mut transaction = client.begin().await?;
    let mut replayed_claims = outbox
        .claim_batch(&mut transaction, owner, 1, lease_duration)
        .await?;
    assert_eq!(replayed_claims.len(), 1);
    let replayed_claim = replayed_claims.remove(0);
    transaction.commit().await?;
    assert_eq!(replayed_claim.attempt(), 1);
    assert!(replayed_claim.lease_generation().get() > current_claim.lease_generation().get());

    let mut transaction = client.begin().await?;
    let stale_after_replay = outbox
        .mark_published(&mut transaction, &current_claim, owner)
        .await;
    assert_eq!(
        stale_after_replay.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    outbox
        .mark_published(&mut transaction, &replayed_claim, owner)
        .await?;
    transaction.commit().await?;

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
    )))
    .execute(&mut client)
    .await?;
    Ok(())
}
