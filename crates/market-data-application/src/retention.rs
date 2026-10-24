//! Retirement is a denial barrier, not a compensating transaction or a lease expiry.
use crate::*;
use edgeagent_contracts::evidence::{DailyEvidenceArtifact, EvidenceId, EvidenceObject};
use std::{fmt, future::Future};

/// Owner-private licensed namespace. Identities confer neither read nor administrative authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionScope {
    /// Dedicated storage boundary.
    pub storage: EvidenceId,
    /// Licensed audience.
    pub audience: EvidenceId,
    /// Logical rights identity; withdrawal covers all its revisions.
    pub rights: EvidenceId,
}

/// Retired request's deletion inventory, retained only until confirmed erasure.
#[derive(Clone)]
pub struct EvidenceCleanup {
    /// Opaque request denial identity, not an evidence fingerprint.
    pub request: EvidenceId,
    /// At most twelve immutable objects, including uncertain writes and unpublished orphans.
    pub objects: Vec<EvidenceObject>,
}
impl fmt::Debug for EvidenceCleanup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvidenceCleanup")
            .field("objects", &self.objects.len())
            .finish_non_exhaustive()
    }
}

/// Trusted maintenance authority, distinct from evidence read permission.
/// Implementations must fence all old writers before confirming request quiescence.
/// A timeout, expired lease or dropped future alone is not confirmation.
pub trait CleanupAuthority: Send + Sync {
    /// Authorize this exact descriptor from the retired request's trusted deletion inventory.
    /// Quiescence alone does not grant permission to erase another scope's object.
    /// # Errors
    /// Deny unknown descriptors, mismatched ownership or an active request.
    fn authorize_deletion(
        &self,
        request: &EvidenceId,
        object: &EvidenceObject,
    ) -> Result<(), BudgetError>;
    /// Confirm all writes for this request have stopped, including delayed backend completion.
    /// # Errors
    /// Deny unknown/unfenced workers; perform no remote I/O inside a database transaction.
    fn confirm_writes_quiescent(&self, request: &EvidenceId) -> Result<(), BudgetError>;
    /// Confirm all provider calls preceding this account block generation are quiescent.
    /// # Errors
    /// Deny absent owner evidence; stale generation confirmation cannot clear a newer block.
    fn confirm_account_quiescent(
        &self,
        account: &EvidenceId,
        generation: i64,
    ) -> Result<(), BudgetError>;
}

/// Durable owner-private inventory and denial checks; no background work or ambient authority.
pub trait EvidenceRetention: Send + Sync {
    /// Register exact object identity before PUT. Reject retired/revoked/expired requests.
    fn stage(
        &self,
        artifact: &DailyEvidenceArtifact,
    ) -> impl Future<Output = Result<(), BudgetError>> + Send;
    /// Recheck current durable denial and original exclusive retention, independently of read grants.
    fn check(
        &self,
        object: &EvidenceObject,
    ) -> impl Future<Output = Result<(), BudgetError>> + Send;
    /// Rotate at most `limit` retired requests with pending deletion, where 1 <= limit <= 16.
    /// Failed/unfenced requests must not starve unrelated cleanup on repeated batches.
    fn pending_cleanup(
        &self,
        limit: u8,
    ) -> impl Future<Output = Result<Vec<EvidenceCleanup>, BudgetError>> + Send;
    /// Forget deletion identifiers only after every listed delete was confirmed with writers fenced.
    fn complete_cleanup(
        &self,
        request: &EvidenceId,
    ) -> impl Future<Output = Result<(), BudgetError>> + Send;
}

/// Physical deletion port; uncertain completion must be retried under the same immutable identity.
pub trait EvidenceDeletion: Send + Sync {
    /// Idempotently erase one exact scoped key after owner-confirmed write quiescence.
    fn delete(
        &self,
        request: &EvidenceId,
        object: &EvidenceObject,
        owner: &impl CleanupAuthority,
    ) -> impl Future<Output = Result<(), EvidenceStorageError>> + Send;
}

/// Compose durable retirement with existing integrity-checked storage. No permissive default exists.
pub struct RetainedEvidenceStorage<'a, S, R> {
    storage: &'a S,
    retention: &'a R,
}
impl<'a, S, R> RetainedEvidenceStorage<'a, S, R> {
    /// Bind owner-private adapters; this constructor performs no I/O.
    #[must_use]
    pub const fn new(storage: &'a S, retention: &'a R) -> Self {
        Self { storage, retention }
    }
}
impl<S, R> fmt::Debug for RetainedEvidenceStorage<'_, S, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetainedEvidenceStorage")
            .finish_non_exhaustive()
    }
}
impl<S: EvidenceStorage, R: EvidenceRetention> EvidenceStorage
    for RetainedEvidenceStorage<'_, S, R>
{
    async fn put(
        &self,
        artifact: &DailyEvidenceArtifact,
        access: &impl EvidenceAccess,
    ) -> Result<EvidenceObject, EvidenceStorageError> {
        self.retention
            .stage(artifact)
            .await
            .map_err(storage_error)?;
        let object = self.storage.put(artifact, access).await?;
        self.retention.check(&object).await.map_err(storage_error)?;
        live(&object, access)?;
        Ok(object)
    }
    async fn get(
        &self,
        object: &EvidenceObject,
        access: &impl EvidenceAccess,
    ) -> Result<DailyEvidenceArtifact, EvidenceStorageError> {
        self.retention.check(object).await.map_err(storage_error)?;
        let artifact = self.storage.get(object, access).await?;
        self.retention.check(object).await.map_err(storage_error)?;
        live(object, access)?;
        Ok(artifact)
    }
}

/// Drain one bounded cleanup batch. Denied/failed/uncertain deletion keeps its inventory for retry.
/// The owner must first commit retirement; this never deletes an active publication or refunds calls.
/// # Errors
/// Preserves maintenance denial, backend uncertainty and inventory confirmation failures.
pub async fn cleanup_evidence(
    retention: &impl EvidenceRetention,
    storage: &impl EvidenceDeletion,
    owner: &impl CleanupAuthority,
    limit: u8,
) -> Result<usize, BudgetError> {
    if !(1..=16).contains(&limit) {
        return Err(BudgetError::new(BudgetErrorKind::Invalid));
    }
    let batch = retention.pending_cleanup(limit).await?;
    if batch.len() > usize::from(limit) {
        return Err(BudgetError::new(BudgetErrorKind::Invalid));
    }
    let mut completed = 0;
    let mut first_error = None;
    for item in batch {
        if item.objects.len() > 12 {
            return Err(BudgetError::new(BudgetErrorKind::Invalid));
        }
        let result = async {
            owner.confirm_writes_quiescent(&item.request)?;
            for object in &item.objects {
                owner.authorize_deletion(&item.request, object)?;
                storage
                    .delete(&item.request, object, owner)
                    .await
                    .map_err(crate::terminal::storage_error)?;
            }
            owner.confirm_writes_quiescent(&item.request)?;
            retention.complete_cleanup(&item.request).await
        }
        .await;
        match result {
            Ok(()) => completed += 1,
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    first_error.map_or(Ok(completed), Err)
}
fn storage_error(error: BudgetError) -> EvidenceStorageError {
    let kind = if error.kind() == BudgetErrorKind::Storage {
        EvidenceStorageErrorKind::Backend
    } else {
        EvidenceStorageErrorKind::Denied
    };
    EvidenceStorageError::with_source(kind, error)
}
fn live(object: &EvidenceObject, access: &impl EvidenceAccess) -> Result<(), EvidenceStorageError> {
    let until = i128::from(object.delete_by.seconds) * 1000
        + i128::from(object.delete_by.nanos / 1_000_000);
    if i128::from(access.now().unix_millis()) >= until {
        return Err(EvidenceStorageError::new(EvidenceStorageErrorKind::Expired));
    }
    access.authorize(object)
}
