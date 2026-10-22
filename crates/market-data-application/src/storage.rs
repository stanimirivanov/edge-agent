//! Owner-private immutable evidence persistence; no public download capability.
use edgeagent_contracts::evidence::{DailyEvidenceArtifact, EvidenceObject};
use edgeagent_market_data_domain::UtcInstant;
use std::{error::Error, fmt, future::Future};

/// Stable storage failure classification, independent from the storage SDK.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EvidenceStorageErrorKind {
    /// Invalid descriptor or operation configuration.
    Invalid,
    /// Current authenticated authority does not permit this exact object.
    Denied,
    /// Exclusive evidence retention deadline has passed.
    Expired,
    /// No object exists under the scoped immutable key.
    NotFound,
    /// Stored bytes, length or embedded scope contradict the expected identity.
    Integrity,
    /// Storage failed; a write may have committed before reporting failure.
    Backend,
    /// Whole-operation deadline elapsed; a write may still have committed.
    Deadline,
}
/// Payload-safe diagnostics. Source inspection is deliberate and may expose SDK details.
pub struct EvidenceStorageError {
    kind: EvidenceStorageErrorKind,
    source: Option<Box<dyn Error + Send + Sync>>,
}
impl EvidenceStorageError {
    /// Construct a classified failure without inventing a lower-level cause.
    #[must_use]
    pub fn new(kind: EvidenceStorageErrorKind) -> Self {
        Self { kind, source: None }
    }
    /// Preserve a real adapter cause, excluded from Display and Debug.
    #[must_use]
    pub fn with_source(
        kind: EvidenceStorageErrorKind,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            source: Some(Box::new(source)),
        }
    }
    /// Inspect the stable classification, not provider-specific text.
    #[must_use]
    pub const fn kind(&self) -> EvidenceStorageErrorKind {
        self.kind
    }
}
impl fmt::Debug for EvidenceStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvidenceStorageError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl fmt::Display for EvidenceStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "evidence storage: {:?}", self.kind)
    }
}
impl Error for EvidenceStorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|e| e as &(dyn Error + 'static))
    }
}
/// Trusted owner boundary, injected per operation; not implementable by remote request data.
/// Resolve the authenticated actor and complete retained admission/scope records, select
/// current rights (including revocations), and deny unknown/conflicting authority.
/// Do not authorize from a digest, caller-supplied audience or historical grant alone.
pub trait EvidenceAccess: Send + Sync {
    /// Current trusted UTC time, sampled again after I/O; never the artifact's capture time.
    fn now(&self) -> UtcInstant;
    /// Check current permission for this exact descriptor and authenticated actor.
    /// # Errors
    /// Return Denied when authenticated scope or current rights do not permit access.
    fn authorize(&self, object: &EvidenceObject) -> Result<(), EvidenceStorageError>;
}
/// Application-owned persistence port. Publication must follow a verified write and
/// the owner's atomic receipt/outbox commit. Dropping an operation cannot undo storage.
/// Retry uncertain writes with the same artifact; never generate a replacement identity.
pub trait EvidenceStorage: Send + Sync {
    /// Create exact canonical bytes, or verify an already-existing identical object.
    /// # Errors
    /// Reject denied/expired access, corrupt collisions, backend failures and deadlines.
    fn put(
        &self,
        artifact: &DailyEvidenceArtifact,
        access: &impl EvidenceAccess,
    ) -> impl Future<Output = Result<EvidenceObject, EvidenceStorageError>> + Send;
    /// Bounded verified retrieval; hashes never grant read permission.
    /// # Errors
    /// Reject invalid identity, denied or expired access, absent or corrupt bytes and I/O failures.
    fn get(
        &self,
        object: &EvidenceObject,
        access: &impl EvidenceAccess,
    ) -> impl Future<Output = Result<DailyEvidenceArtifact, EvidenceStorageError>> + Send;
}
