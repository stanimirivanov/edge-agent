use super::WorkerError;
use edgeagent_contracts::{Component, MessageEnvelope};
use edgeagent_event_spine_conformance::{
    AcceptanceMessage,
    replay::{ReplayCommand, validate_inbound},
};
use edgeagent_inbox_postgres::{PostgresInbox, ReplayRequest as InboundRequest};
use edgeagent_messaging_nats::topology::TopologyPlan;
use edgeagent_outbox_postgres::{
    PostgresOutbox, ReplayDisposition, ReplayRequest as OutboundRequest,
};
use sqlx::{Connection, PgConnection};

pub(super) async fn principal(
    database: &mut PgConnection,
    schema: &str,
    suffix: &str,
) -> Result<String, WorkerError> {
    // session_user identifies the authenticated login, not a caller-supplied
    // actor or SET ROLE/current_user. Search path must identify its owned schema.
    let identity =
        sqlx::query!(r#"SELECT session_user::text AS "actor!", current_schema()::text AS schema"#)
            .fetch_one(&mut *database)
            .await?;
    if identity.actor != format!("{schema}_{suffix}") || identity.schema.as_deref() != Some(schema)
    {
        return Err("replay principal not authorized for owned schema".into());
    }
    Ok(identity.actor)
}

pub(super) async fn authorize(
    database: &mut PgConnection,
    plan: &TopologyPlan,
    component: Component,
    actor: &str,
    request: ReplayCommand,
    rollback: bool,
) -> Result<&'static str, WorkerError> {
    let mut tx = database.begin().await?;
    let outcome = match request {
        ReplayCommand::Inbound {
            request_id,
            consumer_name,
            delivery_key,
            reason,
        } => {
            let evidence = PostgresInbox
                .authorize_quarantine_replay(
                    &mut tx,
                    &consumer_name,
                    &delivery_key,
                    InboundRequest::new(&request_id, actor, &reason)?,
                )
                .await?;
            validate_inbound(
                plan,
                component,
                &consumer_name,
                evidence.transport_subject(),
                evidence.payload(),
            )?;
            sqlx::query!(
                "INSERT INTO conformance_replay_dispatch (request_id, consumer_name, transport_subject, payload) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
                request_id, consumer_name, evidence.transport_subject(), evidence.payload(),
            ).execute(&mut *tx).await?;
            "authorized"
        }
        ReplayCommand::Outbound {
            request_id,
            message_source,
            message_id,
            reason,
        } => {
            let retained = sqlx::query!(
                "SELECT envelope, transport_subject FROM edgeagent_message_outbox WHERE message_source = $1 AND message_id = $2",
                message_source, message_id,
            ).fetch_one(&mut *tx).await?;
            let envelope = MessageEnvelope::from_json(&retained.envelope)?;
            AcceptanceMessage::from_envelope(&envelope)?;
            let runtime = plan
                .runtime_permissions()
                .iter()
                .find(|p| p.component() == Some(component))
                .ok_or("outbound replay scope missing")?;
            if !runtime
                .publish_allow()
                .contains(&retained.transport_subject)
                || !plan.streams().iter().any(|s| {
                    s.definition().message_type() == envelope.message_type()
                        && s.config().subjects.contains(&retained.transport_subject)
                })
            {
                return Err("outbound replay route denied".into());
            }
            match PostgresOutbox
                .replay_quarantined(
                    &mut tx,
                    &message_source,
                    &message_id,
                    OutboundRequest::new(&request_id, actor, &reason)?,
                )
                .await?
            {
                ReplayDisposition::Released => "released",
                ReplayDisposition::AlreadyRequested => "already_released",
            }
        }
    };
    if rollback {
        tx.rollback().await?;
        Ok("rolled_back")
    } else {
        tx.commit().await?;
        Ok(outcome)
    }
}

pub(super) struct Claim {
    pub(super) request: String,
    pub(super) consumer: String,
    pub(super) subject: String,
    pub(super) payload: Vec<u8>,
    pub(super) generation: i64,
}

// Claim deliberately has no derived Debug: it owns the retained untrusted bytes.
pub(super) async fn claim(
    database: &mut PgConnection,
    actor: &str,
    consumer: &str,
) -> Result<Option<Claim>, WorkerError> {
    let mut tx = database.begin().await?;
    let row = sqlx::query!(r#"
        WITH candidate AS (
            SELECT d.request_id FROM conformance_replay_dispatch d
            JOIN edgeagent_message_quarantine_replay_audit a ON a.replay_request_id = d.request_id
            JOIN edgeagent_message_quarantine q ON q.consumer_name = a.consumer_name AND q.delivery_key = a.delivery_key
            WHERE d.published_at IS NULL AND d.attempt_count < 3 AND d.consumer_name = $2
              AND (d.lease_expires_at IS NULL OR d.lease_expires_at <= clock_timestamp())
              AND d.consumer_name = a.consumer_name AND d.transport_subject = q.transport_subject AND d.payload = q.payload
            ORDER BY d.request_id FOR UPDATE OF d SKIP LOCKED LIMIT 1
        )
        UPDATE conformance_replay_dispatch d
        SET attempt_count = attempt_count + 1, generation = generation + 1,
            lease_owner = $1, lease_expires_at = clock_timestamp() + INTERVAL '10 seconds'
        FROM candidate c WHERE d.request_id = c.request_id
        RETURNING d.request_id, d.consumer_name, d.transport_subject, d.payload, d.generation
    "#, actor, consumer).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    Ok(row.map(|row| Claim {
        request: row.request_id,
        consumer: row.consumer_name,
        subject: row.transport_subject,
        payload: row.payload,
        generation: row.generation,
    }))
}

pub(super) async fn complete(
    database: &mut PgConnection,
    actor: &str,
    claim: &Claim,
    receipt: &async_nats::jetstream::publish::PublishAck,
) -> Result<(), WorkerError> {
    let mut tx = database.begin().await?;
    let changed = sqlx::query!(
        "UPDATE conformance_replay_dispatch SET published_at = clock_timestamp(), lease_owner = NULL, lease_expires_at = NULL WHERE request_id = $1 AND lease_owner = $2 AND generation = $3 AND lease_expires_at > clock_timestamp() AND published_at IS NULL",
        claim.request, actor, claim.generation,
    ).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err("replay claim fence lost; publication outcome unresolved".into());
    }
    sqlx::query!(
        "INSERT INTO conformance_replay_receipt (request_id, generation, stream, sequence, duplicate) VALUES ($1, $2, $3, $4, $5)",
        claim.request, claim.generation, receipt.stream, i64::try_from(receipt.sequence)?, receipt.duplicate,
    ).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
