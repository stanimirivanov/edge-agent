use edgeagent_contracts::MessageEnvelope;
use edgeagent_event_spine_conformance::{AcceptanceMessage, ProbeApplied, definitions};
use edgeagent_inbox_handler::{HandlerFailure, HandlerFailureKind};
use edgeagent_inbox_postgres::{PostgresHandlerFuture, PostgresTransactionalMessageHandler};
use edgeagent_messaging::FailureCode;
use edgeagent_outbox_postgres::PostgresOutbox;
use sqlx::{Connection, PgConnection, Postgres, Transaction};

use super::{
    WorkerError, metadata,
    ordering::{Decision, decide},
};

pub(super) async fn accept(
    database: &mut PgConnection,
    envelope: &MessageEnvelope,
    rollback: bool,
) -> Result<(), WorkerError> {
    let mut transaction = database.begin().await?;
    sqlx::query!(
        "INSERT INTO conformance_acceptance (message_source, message_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        envelope.source(), envelope.id(),
    ).execute(&mut *transaction).await?;
    // The outbox's exact-byte comparison rejects changed content even if the
    // acceptance key already exists. Both changes share this single commit.
    PostgresOutbox
        .enqueue(&mut transaction, definitions()[0], envelope)
        .await?;
    if rollback {
        transaction.rollback().await?;
    } else {
        transaction.commit().await?;
    }
    Ok(())
}

pub(super) struct ProbeHandler {
    pub(super) rollback: bool,
}

impl PostgresTransactionalMessageHandler for ProbeHandler {
    fn handle<'handler>(
        &'handler self,
        transaction: &'handler mut Transaction<'_, Postgres>,
        envelope: &'handler MessageEnvelope,
    ) -> PostgresHandlerFuture<'handler> {
        Box::pin(async move {
            let AcceptanceMessage::Command(command) =
                AcceptanceMessage::from_envelope(envelope).map_err(invalid)?
            else {
                return Err(HandlerFailure::permanent(FailureCode::from_static(
                    "unexpected_fact",
                )));
            };
            // A row is created and locked only for this aggregate. Rollback also
            // removes a provisional revision-zero row on a missing predecessor.
            sqlx::query!(
                "INSERT INTO conformance_probe (probe_id, revision, value) VALUES ($1, 0, 0) ON CONFLICT DO NOTHING",
                command.probe_id(),
            )
            .execute(&mut **transaction)
            .await
            .map_err(storage)?;
            let state = sqlx::query!(
                "SELECT revision FROM conformance_probe WHERE probe_id = $1 FOR UPDATE",
                command.probe_id(),
            )
            .fetch_one(&mut **transaction)
            .await
            .map_err(storage)?;
            let receipt = sqlx::query!(
                "SELECT value FROM conformance_command WHERE probe_id = $1 AND expected_revision = $2",
                command.probe_id(), i64::from(command.expected_revision()),
            ).fetch_optional(&mut **transaction).await.map_err(storage)?;
            match decide(
                state.revision,
                command.expected_revision(),
                command.value(),
                receipt.map(|row| row.value),
            ) {
                Decision::Duplicate => return Ok(()),
                Decision::Conflict => {
                    return Err(HandlerFailure::permanent(FailureCode::from_static(
                        "semantic_conflict",
                    )));
                }
                Decision::MissingPredecessor => {
                    return Err(HandlerFailure::transient(FailureCode::from_static(
                        "missing_predecessor",
                    )));
                }
                Decision::Stale => {
                    return Err(HandlerFailure::permanent(FailureCode::from_static(
                        "stale_revision",
                    )));
                }
                Decision::Apply => (),
            }
            // Keep the revision predicate in the write even under the row lock.
            // No path updates an accepted receipt or publishes inside this tx.
            let changed = sqlx::query!(
                "UPDATE conformance_probe SET revision = revision + 1, value = $3 WHERE probe_id = $1 AND revision = $2",
                command.probe_id(), i64::from(command.expected_revision()), i64::from(command.value()),
            ).execute(&mut **transaction).await.map_err(storage)?;
            if changed.rows_affected() != 1 {
                return Err(HandlerFailure::permanent(FailureCode::from_static(
                    "revision_invariant",
                )));
            }
            sqlx::query!(
                "INSERT INTO conformance_command (probe_id, expected_revision, value) VALUES ($1, $2, $3)",
                command.probe_id(), i64::from(command.expected_revision()), i64::from(command.value()),
            ).execute(&mut **transaction).await.map_err(storage)?;
            let revision = command.expected_revision() + 1;
            let mut fact_metadata = metadata(true);
            fact_metadata.id = format!("probe-applied-{}-{revision}", command.probe_id());
            fact_metadata.idempotency_key.clone_from(&fact_metadata.id);
            fact_metadata.subject = format!("probe/{}", command.probe_id());
            fact_metadata
                .partition_key
                .clone_from(&fact_metadata.subject);
            fact_metadata.causation_id = envelope.id().into();
            if let Some(correlation) = envelope.extension("correlationid") {
                fact_metadata.correlation_id = correlation.into();
            }
            // A fact continues the winning command's carrier, not the default
            // fixture's context. Never rewrite the accepted command itself.
            if let Some(parent) = envelope.extension("traceparent") {
                fact_metadata.trace_parent = parent.into();
            }
            fact_metadata.trace_state = envelope.extension("tracestate").map(str::to_owned);
            let fact = ProbeApplied::new(command.probe_id(), revision, command.value())
                .map_err(invalid)?
                .envelope(fact_metadata)
                .map_err(invalid)?;
            PostgresOutbox
                .enqueue(transaction, definitions()[1], &fact)
                .await
                .map_err(storage)?;
            // Synthetic fault is deliberately after all atomic writes. It stays
            // in this tooling callback, never in the production adapter API.
            if self.rollback {
                return Err(HandlerFailure::transient(FailureCode::from_static(
                    "fixture_rollback",
                )));
            }
            Ok(())
        })
    }
}

fn invalid(error: impl std::error::Error + Send + Sync + 'static) -> HandlerFailure {
    HandlerFailure::with_source(
        HandlerFailureKind::Permanent,
        FailureCode::from_static("invalid_probe"),
        error,
    )
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> HandlerFailure {
    HandlerFailure::with_source(
        HandlerFailureKind::Transient,
        FailureCode::from_static("probe_storage"),
        error,
    )
}
