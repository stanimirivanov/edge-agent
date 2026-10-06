//! PostgreSQL adapter for atomic inbound message processing.

use crate::{
    DeliveryDisposition as PostgresDeliveryDisposition, InboxError, InboxErrorKind, PostgresInbox,
    QuarantineDisposition as PostgresQuarantineDisposition,
    QuarantineEvidence as PostgresQuarantineEvidence,
};
use edgeagent_contracts::{MessageEnvelope, MessageRegistry};
use edgeagent_messaging::{
    HandlerFailure, InboundMessageStore, InboundProcessingError, InboundQuarantine,
    InboxDisposition, InboxFuture, InboxStoreError, InboxStoreErrorKind, QuarantineDisposition,
};
use sqlx::{Connection, PgConnection, Postgres, Transaction};
use std::future::Future;
use std::pin::Pin;

/// Future returned by service-owned PostgreSQL transactional work.
pub type PostgresHandlerFuture<'handler> =
    Pin<Box<dyn Future<Output = Result<(), HandlerFailure>> + Send + 'handler>>;

/// Infrastructure-side callback for service-owned work in the inbox transaction.
///
/// Implementations may write service-owned tables and enqueue outbox messages.
/// They should delegate business decisions to deterministic application/domain
/// code; the PostgreSQL transaction is deliberately confined to this adapter API.
/// The current `edgeagent-outbox-postgres` helper uses a different driver and
/// cannot join this SQLx transaction; an emitting consumer needs a compatible
/// outbox write path before it can claim atomic inbox-plus-outbox effects.
pub trait PostgresTransactionalMessageHandler: Sync {
    /// Apply a first delivery's service-owned transition inside the transaction.
    ///
    /// Implementations MUST write only service-owned tables, MUST NOT perform
    /// network calls or other effects that cannot roll back with PostgreSQL,
    /// and MUST return before the adapter commits the transaction.
    fn handle<'handler>(
        &'handler self,
        transaction: &'handler mut Transaction<'_, Postgres>,
        envelope: &'handler MessageEnvelope,
    ) -> PostgresHandlerFuture<'handler>;
}

/// PostgreSQL implementation of the atomic inbound application port.
pub struct PostgresInboundMessageStore<'client, 'handler> {
    client: &'client mut PgConnection,
    handler: &'handler dyn PostgresTransactionalMessageHandler,
}

impl<'client, 'handler> PostgresInboundMessageStore<'client, 'handler> {
    /// Bind one database client and service-owned transactional callback.
    #[must_use]
    pub const fn new(
        client: &'client mut PgConnection,
        handler: &'handler dyn PostgresTransactionalMessageHandler,
    ) -> Self {
        Self { client, handler }
    }
}

impl InboundMessageStore for PostgresInboundMessageStore<'_, '_> {
    fn process<'operation>(
        &'operation mut self,
        consumer_name: &'operation str,
        registry: &'operation MessageRegistry<'_>,
        envelope: &'operation MessageEnvelope,
    ) -> InboxFuture<'operation, Result<InboxDisposition, InboundProcessingError>> {
        Box::pin(async move {
            let mut transaction = self
                .client
                .begin()
                .await
                .map_err(|error| InboundProcessingError::Store(classify_driver_error(error)))?;
            let disposition = match PostgresInbox
                .record_delivery(&mut transaction, consumer_name, registry, envelope)
                .await
            {
                Ok(disposition) => disposition,
                Err(error) => {
                    let error = map_inbox_error(error);
                    if let Err(rollback_error) = transaction.rollback().await {
                        return Err(InboundProcessingError::Store(unavailable(rollback_error)));
                    }
                    return Err(InboundProcessingError::Store(error));
                }
            };

            match disposition {
                PostgresDeliveryDisposition::Duplicate => {
                    transaction.commit().await.map_err(|error| {
                        InboundProcessingError::Store(classify_driver_error(error))
                    })?;
                    Ok(InboxDisposition::Duplicate)
                }
                PostgresDeliveryDisposition::FirstDelivery => {
                    if let Err(error) = self.handler.handle(&mut transaction, envelope).await {
                        if let Err(rollback_error) = transaction.rollback().await {
                            return Err(InboundProcessingError::Store(unavailable(rollback_error)));
                        }
                        return Err(InboundProcessingError::Handler(error));
                    }
                    transaction.commit().await.map_err(|error| {
                        InboundProcessingError::Store(classify_driver_error(error))
                    })?;
                    Ok(InboxDisposition::Applied)
                }
            }
        })
    }

    fn quarantine<'operation>(
        &'operation mut self,
        consumer_name: &'operation str,
        evidence: InboundQuarantine<'operation>,
    ) -> InboxFuture<'operation, Result<QuarantineDisposition, InboxStoreError>> {
        Box::pin(async move {
            let postgres_evidence = PostgresQuarantineEvidence::new(
                evidence.delivery_key(),
                evidence.transport_subject(),
                evidence.delivery_attempt(),
                evidence.payload(),
                evidence.failure_code(),
            )
            .map_err(map_inbox_error)?;
            let mut transaction = self.client.begin().await.map_err(classify_driver_error)?;
            let disposition = match PostgresInbox
                .quarantine_delivery(&mut transaction, consumer_name, postgres_evidence)
                .await
            {
                Ok(disposition) => disposition,
                Err(error) => {
                    let error = map_inbox_error(error);
                    if let Err(rollback_error) = transaction.rollback().await {
                        return Err(unavailable(rollback_error));
                    }
                    return Err(error);
                }
            };
            transaction.commit().await.map_err(classify_driver_error)?;
            Ok(match disposition {
                PostgresQuarantineDisposition::Inserted => QuarantineDisposition::Inserted,
                PostgresQuarantineDisposition::AlreadyPresent => {
                    QuarantineDisposition::AlreadyPresent
                }
            })
        })
    }
}

fn unavailable(error: sqlx::Error) -> InboxStoreError {
    // A failed rollback cannot prove that the original operation was undone.
    // Preserve the cause without exposing driver diagnostics through Debug.
    map_inbox_error(InboxError::ambiguous_storage(error))
}

fn classify_driver_error(error: sqlx::Error) -> InboxStoreError {
    map_inbox_error(InboxError::storage(error))
}

fn map_inbox_error(error: InboxError) -> InboxStoreError {
    let kind = match error.kind() {
        InboxErrorKind::Contract => InboxStoreErrorKind::Contract,
        InboxErrorKind::MessageIdentityConflict => InboxStoreErrorKind::MessageIdentityConflict,
        InboxErrorKind::Storage => InboxStoreErrorKind::Unavailable,
        InboxErrorKind::InvalidConsumerName
        | InboxErrorKind::InvalidQuarantineEvidence
        | InboxErrorKind::QuarantineIdentityConflict
        | InboxErrorKind::InvalidReplayRequest
        | InboxErrorKind::ReplayRequestConflict
        | InboxErrorKind::NotQuarantined
        | InboxErrorKind::StorageInvariant => InboxStoreErrorKind::Invariant,
    };
    InboxStoreError::with_source(kind, error)
}

#[cfg(test)]
mod tests {
    use super::{InboxStoreErrorKind, map_inbox_error};
    use crate::InboxError;

    #[test]
    fn adapter_preserves_identity_and_invariant_categories() {
        assert_eq!(
            map_inbox_error(InboxError::message_identity_conflict()).kind(),
            InboxStoreErrorKind::MessageIdentityConflict
        );
        assert_eq!(
            map_inbox_error(InboxError::storage_invariant()).kind(),
            InboxStoreErrorKind::Invariant
        );
    }
}
