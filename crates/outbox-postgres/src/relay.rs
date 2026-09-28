//! PostgreSQL adapter for the application-owned outbound relay storage port.

use crate::{OutboxError, OutboxErrorKind, PostgresOutbox};
use edgeagent_messaging::{
    ClaimedMessage, OutboxRelayStore, OutboxStoreError, OutboxStoreErrorKind, OutboxStoreFuture,
};
use std::time::Duration;
use tokio_postgres::Client;

/// PostgreSQL-backed outbound relay storage adapter.
pub struct PostgresOutboxRelay<'client> {
    client: &'client mut Client,
}

impl<'client> PostgresOutboxRelay<'client> {
    /// Bind the adapter to one service-owned PostgreSQL client.
    #[must_use]
    pub const fn new(client: &'client mut Client) -> Self {
        Self { client }
    }
}

impl OutboxRelayStore for PostgresOutboxRelay<'_> {
    fn claim_one<'operation>(
        &'operation mut self,
        lease_owner: &'operation str,
        lease_duration: Duration,
    ) -> OutboxStoreFuture<'operation, Option<ClaimedMessage>> {
        Box::pin(async move {
            let transaction = self.client.transaction().await.map_err(unavailable)?;
            let mut messages = PostgresOutbox
                .claim_batch(&transaction, lease_owner, 1, lease_duration)
                .await
                .map_err(operation)?;
            transaction.commit().await.map_err(unavailable)?;
            if messages.len() > 1 {
                return Err(OutboxStoreError::with_source(
                    OutboxStoreErrorKind::Invariant,
                    AdapterInvariant("single-message claim returned more than one record"),
                ));
            }
            Ok(messages.pop())
        })
    }

    fn mark_published<'operation>(
        &'operation mut self,
        message_source: &'operation str,
        message_id: &'operation str,
        lease_owner: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            let transaction = self.client.transaction().await.map_err(unavailable)?;
            PostgresOutbox
                .mark_published(&transaction, message_source, message_id, lease_owner)
                .await
                .map_err(operation)?;
            transaction.commit().await.map_err(unavailable)
        })
    }

    fn release_for_retry<'operation>(
        &'operation mut self,
        message_source: &'operation str,
        message_id: &'operation str,
        lease_owner: &'operation str,
        retry_after: Duration,
        failure_code: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            let transaction = self.client.transaction().await.map_err(unavailable)?;
            PostgresOutbox
                .release_for_retry(
                    &transaction,
                    message_source,
                    message_id,
                    lease_owner,
                    retry_after,
                    failure_code,
                )
                .await
                .map_err(operation)?;
            transaction.commit().await.map_err(unavailable)
        })
    }

    fn quarantine<'operation>(
        &'operation mut self,
        message_source: &'operation str,
        message_id: &'operation str,
        lease_owner: &'operation str,
        reason: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            let transaction = self.client.transaction().await.map_err(unavailable)?;
            PostgresOutbox
                .quarantine(
                    &transaction,
                    message_source,
                    message_id,
                    lease_owner,
                    reason,
                )
                .await
                .map_err(operation)?;
            transaction.commit().await.map_err(unavailable)
        })
    }
}

fn unavailable(error: tokio_postgres::Error) -> OutboxStoreError {
    OutboxStoreError::with_source(OutboxStoreErrorKind::Unavailable, error)
}

fn operation(error: OutboxError) -> OutboxStoreError {
    let kind = match error.kind() {
        OutboxErrorKind::Storage => OutboxStoreErrorKind::Unavailable,
        OutboxErrorKind::StorageInvariant => OutboxStoreErrorKind::Invariant,
        OutboxErrorKind::Contract
        | OutboxErrorKind::MessageIdentityConflict
        | OutboxErrorKind::InvalidArgument
        | OutboxErrorKind::LeaseLost
        | OutboxErrorKind::ReplayRequestConflict
        | OutboxErrorKind::NotQuarantined => OutboxStoreErrorKind::StateTransition,
    };
    OutboxStoreError::with_source(kind, error)
}

#[derive(Debug)]
struct AdapterInvariant(&'static str);

impl std::fmt::Display for AdapterInvariant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for AdapterInvariant {}

#[cfg(test)]
mod tests {
    use super::{OutboxStoreErrorKind, operation};
    use crate::OutboxError;

    #[test]
    fn adapter_preserves_invariant_and_state_transition_categories() {
        assert_eq!(
            operation(OutboxError::storage_invariant()).kind(),
            OutboxStoreErrorKind::Invariant
        );
        assert_eq!(
            operation(OutboxError::lease_lost()).kind(),
            OutboxStoreErrorKind::StateTransition
        );
    }
}
