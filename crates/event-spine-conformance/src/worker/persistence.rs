use edgeagent_contracts::MessageEnvelope;
use edgeagent_event_spine_conformance::{AcceptanceMessage, ProbeApplied, definitions};
use edgeagent_inbox_handler::{HandlerFailure, HandlerFailureKind};
use edgeagent_inbox_postgres::{PostgresHandlerFuture, PostgresTransactionalMessageHandler};
use edgeagent_messaging::FailureCode;
use edgeagent_outbox_postgres::PostgresOutbox;
use sqlx::{Connection, PgConnection, Postgres, Transaction};

use super::{WorkerError, metadata};

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
            // This slice supports initial acceptance only. Revision sequencing
            // and semantic business-key duplicate policy belong to slice 4.
            if command.expected_revision() != 0 {
                return Err(HandlerFailure::permanent(FailureCode::from_static(
                    "revision_unsupported",
                )));
            }
            sqlx::query!(
                "INSERT INTO conformance_probe (probe_id, revision, value) VALUES ($1, 1, $2)",
                command.probe_id(),
                i64::from(command.value()),
            )
            .execute(&mut **transaction)
            .await
            .map_err(storage)?;
            let fact = ProbeApplied::new(command.probe_id(), 1, command.value())
                .map_err(invalid)?
                .envelope(metadata(true))
                .map_err(invalid)?;
            PostgresOutbox
                .enqueue(transaction, definitions()[1], &fact)
                .await
                .map_err(storage)?;
            // Synthetic fault is deliberately after all three writes. It stays
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
