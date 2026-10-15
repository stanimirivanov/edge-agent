//! PostgreSQL adapter for the application-owned outbound relay storage port.

use crate::{OutboxError, OutboxErrorKind, PostgresOutbox};
use edgeagent_messaging::{
    ClaimedMessage, FailureCode, LeaseDuration, OutboxRelayStore, OutboxRetryDelay,
    OutboxStoreError, OutboxStoreErrorKind, OutboxStoreFuture,
};
use sqlx::{Connection, PgConnection, Postgres, Transaction};

/// PostgreSQL-backed outbound relay storage adapter.
///
/// Each operation owns a top-level transaction and returns success only after
/// commit. An active caller-owned SQLx transaction is rejected as `Invariant`
/// before another transaction is constructed; neither its writes nor its
/// transaction boundary are changed. Use SQLx's transaction API to control this
/// connection: raw SQL `BEGIN`/`COMMIT` bypasses SQLx's transaction tracking and
/// is unsupported.
///
/// # Cancellation
///
/// Once a transaction has begun, dropping an operation before commit queues
/// SQLx rollback; it does not prove rollback has completed. Subsequent
/// asynchronous connection use processes pending protocol work. Cancellation
/// during commit confirmation can leave a
/// committed transition. Recover through persisted state and the original claim
/// fence, not automatic compensation. An abandoned committed claim becomes
/// eligible after lease expiry. See [`OutboxRelayStore`] for each operation's
/// recovery contract.
pub struct PostgresOutboxRelay<'client> {
    client: &'client mut PgConnection,
}

impl<'client> PostgresOutboxRelay<'client> {
    /// Bind the adapter to one service-owned PostgreSQL client.
    ///
    /// Every operation rejects an active SQLx-managed transaction; passing a
    /// connection borrowed from a producer transaction cannot make relay work
    /// part of that transaction. Producers still use [`PostgresOutbox::enqueue`]
    /// directly with their caller-owned transaction.
    #[must_use]
    pub const fn new(client: &'client mut PgConnection) -> Self {
        Self { client }
    }
}

impl std::fmt::Debug for PostgresOutboxRelay<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The connection can contain credentials and driver-specific state.
        formatter
            .debug_struct("PostgresOutboxRelay")
            .finish_non_exhaustive()
    }
}

impl OutboxRelayStore for PostgresOutboxRelay<'_> {
    fn claim_one<'operation>(
        &'operation mut self,
        lease_owner: &'operation str,
        lease_duration: LeaseDuration,
    ) -> OutboxStoreFuture<'operation, Option<ClaimedMessage>> {
        Box::pin(async move {
            let mut transaction = begin_top_level(self.client).await?;
            let mut messages = PostgresOutbox
                .claim_batch(&mut transaction, lease_owner, 1, lease_duration)
                .await
                .map_err(operation)?;
            if messages.len() > 1 {
                return Err(OutboxStoreError::new(OutboxStoreErrorKind::Invariant));
            }
            transaction.commit().await.map_err(database)?;
            Ok(messages.pop())
        })
    }

    fn mark_published<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        lease_owner: &'operation str,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            let mut transaction = begin_top_level(self.client).await?;
            PostgresOutbox
                .mark_published(&mut transaction, claim, lease_owner)
                .await
                .map_err(operation)?;
            transaction.commit().await.map_err(database)
        })
    }

    fn release_for_retry<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        lease_owner: &'operation str,
        retry_after: OutboxRetryDelay,
        failure_code: FailureCode,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            let mut transaction = begin_top_level(self.client).await?;
            PostgresOutbox
                .release_for_retry(
                    &mut transaction,
                    claim,
                    lease_owner,
                    retry_after,
                    failure_code,
                )
                .await
                .map_err(operation)?;
            transaction.commit().await.map_err(database)
        })
    }

    fn quarantine<'operation>(
        &'operation mut self,
        claim: &'operation ClaimedMessage,
        lease_owner: &'operation str,
        reason: FailureCode,
    ) -> OutboxStoreFuture<'operation, ()> {
        Box::pin(async move {
            let mut transaction = begin_top_level(self.client).await?;
            PostgresOutbox
                .quarantine(&mut transaction, claim, lease_owner, reason)
                .await
                .map_err(operation)?;
            transaction.commit().await.map_err(database)
        })
    }
}

async fn begin_top_level(
    client: &mut PgConnection,
) -> Result<Transaction<'_, Postgres>, OutboxStoreError> {
    // Reject before constructing a Transaction: begin() would create a savepoint,
    // and a failed custom begin can itself queue rollback of the caller's work.
    if client.is_in_transaction() {
        return Err(OutboxStoreError::new(OutboxStoreErrorKind::Invariant));
    }
    client.begin().await.map_err(database)
}

fn database(error: sqlx::Error) -> OutboxStoreError {
    operation(OutboxError::storage(error))
}

fn operation(error: OutboxError) -> OutboxStoreError {
    let kind = match error.kind() {
        OutboxErrorKind::Storage => OutboxStoreErrorKind::Unavailable,
        OutboxErrorKind::LeaseLost => OutboxStoreErrorKind::StateTransition,
        OutboxErrorKind::StorageInvariant
        | OutboxErrorKind::InvalidArgument
        | OutboxErrorKind::Contract
        | OutboxErrorKind::MessageIdentityConflict
        | OutboxErrorKind::ReplayRequestConflict
        | OutboxErrorKind::NotQuarantined => OutboxStoreErrorKind::Invariant,
    };
    OutboxStoreError::with_source(kind, error)
}

#[cfg(test)]
mod tests;
