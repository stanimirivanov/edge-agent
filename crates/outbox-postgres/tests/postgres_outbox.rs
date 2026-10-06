//! Opt-in conformance test against the checked-in local PostgreSQL profile.

use edgeagent_contracts::{
    Component, EventRetention, MessageDefinition, MessageMetadata, MessageRegistry,
};
use edgeagent_outbox_postgres::{
    EnqueueDisposition, OutboxErrorKind, PostgresOutbox, ReplayDisposition, ReplayRequest,
};
use serde_json::json;
use std::env;
use std::error::Error;
use std::process;
use std::time::Duration;
use tokio_postgres::NoTls;

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
    let (mut client, connection) = tokio_postgres::connect(&postgres_url, NoTls).await?;
    let connection_task = tokio::spawn(connection);
    let schema = format!("edgeagent_outbox_test_{}", process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .await?;
    for migration in PostgresOutbox::MIGRATIONS {
        client.batch_execute(migration).await?;
    }

    let outbox = PostgresOutbox;
    let gateway_envelope = COMMAND.build(metadata(Component::Gateway), &json!({"quantity": 1}))?;
    let research_envelope =
        COMMAND.build(metadata(Component::Research), &json!({"quantity": 1}))?;

    let transaction = client.transaction().await?;
    assert_eq!(
        outbox
            .enqueue(&transaction, COMMAND, &gateway_envelope)
            .await?,
        EnqueueDisposition::Inserted
    );
    transaction.rollback().await?;
    let row = client
        .query_one("SELECT count(*) FROM edgeagent_message_outbox", &[])
        .await?;
    assert_eq!(row.try_get::<_, i64>(0)?, 0);

    let transaction = client.transaction().await?;
    assert_eq!(
        outbox
            .enqueue(&transaction, COMMAND, &gateway_envelope)
            .await?,
        EnqueueDisposition::Inserted
    );
    assert_eq!(
        outbox
            .enqueue(&transaction, COMMAND, &gateway_envelope)
            .await?,
        EnqueueDisposition::AlreadyPresent
    );
    assert_eq!(
        outbox
            .enqueue(&transaction, COMMAND, &research_envelope)
            .await?,
        EnqueueDisposition::Inserted
    );
    transaction.commit().await?;

    let conflicting = COMMAND.build(metadata(Component::Gateway), &json!({"quantity": 2}))?;
    let transaction = client.transaction().await?;
    let conflict = outbox.enqueue(&transaction, COMMAND, &conflicting).await;
    assert_eq!(
        conflict.err().map(|error| error.kind()),
        Some(OutboxErrorKind::MessageIdentityConflict)
    );
    let stored = transaction
        .query_one(
            "SELECT envelope FROM edgeagent_message_outbox WHERE message_source = $1 AND message_id = $2",
            &[&gateway_envelope.source(), &gateway_envelope.id()],
        )
        .await?;
    let stored_bytes: Vec<u8> = stored.try_get("envelope")?;
    assert_eq!(stored_bytes, gateway_envelope.to_json()?);
    transaction.rollback().await?;

    let transaction = client.transaction().await?;
    let claimed = outbox
        .claim_batch(&transaction, "relay_01", 10, Duration::from_secs(30))
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
    let transaction = client.transaction().await?;
    let lost = outbox.mark_published(&transaction, first, "relay_02").await;
    assert_eq!(
        lost.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    transaction.rollback().await?;

    let transaction = client.transaction().await?;
    outbox
        .release_for_retry(
            &transaction,
            first,
            "relay_01",
            Duration::ZERO,
            "transport_unavailable",
        )
        .await?;
    outbox
        .mark_published(&transaction, second, "relay_01")
        .await?;
    transaction.commit().await?;

    let transaction = client.transaction().await?;
    let retried = outbox
        .claim_batch(&transaction, "relay_02", 10, Duration::from_secs(30))
        .await?;
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].attempt(), 2);
    outbox
        .quarantine(&transaction, &retried[0], "relay_02", "transport_rejected")
        .await?;
    transaction.commit().await?;

    let transaction = client.transaction().await?;
    assert!(
        outbox
            .claim_batch(&transaction, "relay_03", 10, Duration::from_secs(30))
            .await?
            .is_empty()
    );
    transaction.commit().await?;

    let replay_request = ReplayRequest::new(
        "replay-request-01",
        "urn:edgeagent:operator:alice",
        "configuration_remediated",
    )?;
    let transaction = client.transaction().await?;
    assert_eq!(
        outbox
            .replay_quarantined(
                &transaction,
                retried[0].message_source(),
                retried[0].message_id(),
                replay_request,
            )
            .await?,
        ReplayDisposition::Released
    );
    transaction.commit().await?;

    let transaction = client.transaction().await?;
    assert_eq!(
        outbox
            .replay_quarantined(
                &transaction,
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
            &transaction,
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
            &transaction,
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

    let transaction = client.transaction().await?;
    let replayed = outbox
        .claim_batch(&transaction, "relay_03", 10, Duration::from_secs(30))
        .await?;
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].attempt(), 1);
    assert_eq!(replayed[0].envelope_bytes(), retried[0].envelope_bytes());
    outbox
        .quarantine(&transaction, &replayed[0], "relay_03", "transport_rejected")
        .await?;
    transaction.commit().await?;

    let transaction = client.transaction().await?;
    assert_eq!(
        outbox
            .replay_quarantined(
                &transaction,
                replayed[0].message_source(),
                replayed[0].message_id(),
                replay_request,
            )
            .await?,
        ReplayDisposition::AlreadyRequested
    );
    transaction.commit().await?;

    let transaction = client.transaction().await?;
    assert!(
        outbox
            .claim_batch(&transaction, "relay_04", 10, Duration::from_secs(30))
            .await?
            .is_empty()
    );
    transaction.commit().await?;

    let transaction = client.transaction().await?;
    assert_eq!(
        outbox
            .replay_quarantined(
                &transaction,
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

    let transaction = client.transaction().await?;
    let replayed_again = outbox
        .claim_batch(&transaction, "relay_04", 10, Duration::from_secs(30))
        .await?;
    assert_eq!(replayed_again.len(), 1);
    assert_eq!(replayed_again[0].attempt(), 1);
    assert_eq!(
        replayed_again[0].envelope_bytes(),
        retried[0].envelope_bytes()
    );
    outbox
        .mark_published(&transaction, &replayed_again[0], "relay_04")
        .await?;
    transaction.commit().await?;

    let audit = client
        .query_one(
            "SELECT requested_by, reason, prior_quarantine_reason, prior_attempt_count FROM edgeagent_message_outbox_replay_audit WHERE replay_request_id = $1",
            &[&"replay-request-01"],
        )
        .await?;
    assert_eq!(
        audit.try_get::<_, String>(0)?,
        "urn:edgeagent:operator:alice"
    );
    assert_eq!(audit.try_get::<_, String>(1)?, "configuration_remediated");
    assert_eq!(audit.try_get::<_, String>(2)?, "transport_rejected");
    assert_eq!(audit.try_get::<_, i32>(3)?, 2);
    let audit_count = client
        .query_one(
            "SELECT count(*) FROM edgeagent_message_outbox_replay_audit",
            &[],
        )
        .await?;
    assert_eq!(audit_count.try_get::<_, i64>(0)?, 2);

    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .await?;
    connection_task.abort();
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn same_owner_reclaim_rejects_stale_transitions_and_preserves_replay_fence()
-> Result<(), Box<dyn Error>> {
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let (mut client, connection) = tokio_postgres::connect(&postgres_url, NoTls).await?;
    let connection_task = tokio::spawn(connection);
    let schema = format!("edgeagent_outbox_fence_test_{}", process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .await?;
    for migration in PostgresOutbox::MIGRATIONS {
        client.batch_execute(migration).await?;
    }

    let outbox = PostgresOutbox;
    let envelope = COMMAND.build(metadata(Component::Gateway), &json!({"quantity": 1}))?;
    let transaction = client.transaction().await?;
    assert_eq!(
        outbox.enqueue(&transaction, COMMAND, &envelope).await?,
        EnqueueDisposition::Inserted
    );
    transaction.commit().await?;

    let owner = "relay_same_owner";
    let lease_duration = Duration::from_secs(15 * 60);
    let transaction = client.transaction().await?;
    let mut first_claims = outbox
        .claim_batch(&transaction, owner, 1, lease_duration)
        .await?;
    assert_eq!(first_claims.len(), 1);
    let first_claim = first_claims.remove(0);
    transaction.commit().await?;
    assert_eq!(first_claim.attempt(), 1);

    // Expire only the first claim without sleeping or depending on scheduler timing.
    let first_generation = i64::try_from(first_claim.lease_generation().get())?;
    let expired = client
        .execute(
            "UPDATE edgeagent_message_outbox SET lease_expires_at = clock_timestamp() - INTERVAL '1 millisecond' WHERE message_source = $1 AND message_id = $2 AND lease_owner = $3 AND lease_generation = $4",
            &[
                &first_claim.message_source(),
                &first_claim.message_id(),
                &owner,
                &first_generation,
            ],
        )
        .await?;
    assert_eq!(expired, 1);

    let transaction = client.transaction().await?;
    let mut current_claims = outbox
        .claim_batch(&transaction, owner, 1, lease_duration)
        .await?;
    assert_eq!(current_claims.len(), 1);
    let current_claim = current_claims.remove(0);
    transaction.commit().await?;
    assert_eq!(current_claim.attempt(), 2);
    assert!(current_claim.lease_generation().get() > first_claim.lease_generation().get());

    let transaction = client.transaction().await?;
    let stale_publish = outbox
        .mark_published(&transaction, &first_claim, owner)
        .await;
    assert_eq!(
        stale_publish.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    let stale_retry = outbox
        .release_for_retry(
            &transaction,
            &first_claim,
            owner,
            Duration::ZERO,
            "transport_unavailable",
        )
        .await;
    assert_eq!(
        stale_retry.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    let stale_quarantine = outbox
        .quarantine(&transaction, &first_claim, owner, "transport_rejected")
        .await;
    assert_eq!(
        stale_quarantine.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    transaction.commit().await?;

    let state = client
        .query_one(
            "SELECT lease_owner, lease_generation, attempt_count, published_at IS NULL, quarantined_at IS NULL, last_failure_code, lease_expires_at > clock_timestamp() FROM edgeagent_message_outbox WHERE message_source = $1 AND message_id = $2",
            &[&current_claim.message_source(), &current_claim.message_id()],
        )
        .await?;
    assert_eq!(
        state.try_get::<_, Option<String>>(0)?.as_deref(),
        Some(owner)
    );
    assert_eq!(
        state.try_get::<_, i64>(1)?,
        i64::try_from(current_claim.lease_generation().get())?
    );
    assert_eq!(state.try_get::<_, i32>(2)?, 2);
    assert!(state.try_get::<_, bool>(3)?);
    assert!(state.try_get::<_, bool>(4)?);
    assert_eq!(state.try_get::<_, Option<String>>(5)?, None);
    assert!(state.try_get::<_, bool>(6)?);

    let transaction = client.transaction().await?;
    outbox
        .quarantine(&transaction, &current_claim, owner, "transport_rejected")
        .await?;
    transaction.commit().await?;

    let replay_request = ReplayRequest::new(
        "fence-replay-01",
        "urn:edgeagent:operator:alice",
        "configuration_remediated",
    )?;
    let transaction = client.transaction().await?;
    assert_eq!(
        outbox
            .replay_quarantined(
                &transaction,
                current_claim.message_source(),
                current_claim.message_id(),
                replay_request,
            )
            .await?,
        ReplayDisposition::Released
    );
    transaction.commit().await?;

    let released = client
        .query_one(
            "SELECT lease_generation, attempt_count, quarantined_at IS NULL FROM edgeagent_message_outbox WHERE message_source = $1 AND message_id = $2",
            &[&current_claim.message_source(), &current_claim.message_id()],
        )
        .await?;
    assert_eq!(
        released.try_get::<_, i64>(0)?,
        i64::try_from(current_claim.lease_generation().get())?
    );
    assert_eq!(released.try_get::<_, i32>(1)?, 0);
    assert!(released.try_get::<_, bool>(2)?);

    let transaction = client.transaction().await?;
    let mut replayed_claims = outbox
        .claim_batch(&transaction, owner, 1, lease_duration)
        .await?;
    assert_eq!(replayed_claims.len(), 1);
    let replayed_claim = replayed_claims.remove(0);
    transaction.commit().await?;
    assert_eq!(replayed_claim.attempt(), 1);
    assert!(replayed_claim.lease_generation().get() > current_claim.lease_generation().get());

    let transaction = client.transaction().await?;
    let stale_after_replay = outbox
        .mark_published(&transaction, &current_claim, owner)
        .await;
    assert_eq!(
        stale_after_replay.err().map(|error| error.kind()),
        Some(OutboxErrorKind::LeaseLost)
    );
    outbox
        .mark_published(&transaction, &replayed_claim, owner)
        .await?;
    transaction.commit().await?;

    client
        .batch_execute(&format!(
            "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
        ))
        .await?;
    connection_task.abort();
    Ok(())
}
