//! Vendor-neutral acquisition admission and response conformance, not a worker or HTTP client.
//! Review catalogs must be authenticated outside this crate. Tokens are short-lived checks,
//! not credentials; recheck current catalogs before accepting results or using retained data.
#![deny(missing_docs)]
mod activation;
mod supervision;
pub use supervision::{
    RejectedStreamObservation, StreamDispatch, StreamLaneStatus, StreamStop, StreamSupervisor,
    SupervisedSource, SupervisionError,
};
mod streaming;
pub use streaming::read_stream_publication;
pub use streaming::verify_stream_capture_record;
pub use streaming::{
    CapturedStreamObservation, StreamCaptureBoundary, StreamCleanup, StreamDeletion,
    StreamDeletionAuthority, StreamNamespaceClaim, StreamOwnerInputs, StreamPublication,
    StreamPublicationLedger, StreamRecoveryLedger, StreamRecoveryState, StreamRestore,
    StreamSegmentInput, StreamingAccountPolicy, StreamingArrival, StreamingAuthority,
    StreamingConnection, StreamingFrame, StreamingLedger, StreamingLimits, StreamingSession,
    StreamingSessionData, StreamingUsage, freeze_stream_segment, publish_stream_segment,
    read_stream_segment, recover_pending_stream_segment,
};
mod canonical;
pub use activation::{
    QualificationAccountState, QualificationAssessment, QualificationAuthority,
    QualificationControl, QualificationControlReview, QualificationPreflight, QualificationProfile,
    QualificationProfileData, QualificationProfileFormat, QualificationProvider,
    preflight_qualification,
};
mod native_evidence;
pub use native_evidence::{NativeEvidenceFreeze, freeze_native_evidence};
mod admission;
mod authority;
mod budget;
mod date_only;
mod date_only_source;
mod precise_daily;
pub use date_only_source::DateOnlySchedule;
pub use precise_daily::{
    DailyObservationSlot, DailyPageReceipt, DailySourceCapture, PreciseDailyRow,
    PreciseDailySchedule,
};
mod dispatch;
pub use admission::{AcquisitionLimits, AdmissionRecord};
pub use budget::{
    AccountBudget, AcquisitionBudget, AcquisitionLedger, BudgetError, BudgetErrorKind,
    BudgetSnapshot, CallCompletion, CallPermit, DailyCreditPolicy, DailyCreditUsage,
};
pub use date_only::freeze_date_only_time;
pub use dispatch::{AcquisitionUsage, DispatchAuthority, DispatchCheck};
mod evidence;
mod provider;
mod request;
mod selection;
pub use selection::{
    EarlyRouter, FrameKind, FrameMetadata, InstrumentRoute, InstrumentSelection, RouteBinding,
    RouteDecision, RoutingDirectory, RoutingKey, RoutingScope, SelectedDailyProvider,
    SelectedRequest, SelectionContext, SelectionError, SelectionPlan,
};
mod retention;
mod storage;
pub use retention::{
    CleanupAuthority, EvidenceCleanup, EvidenceDeletion, EvidenceRetention,
    NativeEvidenceRetention, RetainedEvidenceStorage, RetentionScope, cleanup_evidence,
};
mod terminal;
pub use authority::{
    AccessScope, Permission, ProviderReview, ReviewStatus, RightsRecord, UsePermissions,
};
use edgeagent_market_data_domain::{ReferenceHistory, ReferenceVersion, ValueError};
pub use evidence::{EvidenceFreeze, EvidenceFreezeError, freeze_evidence};
pub use provider::{DailyProvider, ProviderBatch, ProviderFailure, validate_batch};
pub use request::{AcquisitionRequest, Cancellation, KnowledgeMode, RawRetention, RequestBudget};
use std::num::NonZeroU128;
pub use storage::{
    EvidenceAccess, EvidenceStorage, EvidenceStorageError, EvidenceStorageErrorKind,
    NativeEvidenceStorage, StreamSegmentAccess, StreamSegmentStorage,
};
pub use terminal::{
    AcquisitionPublication, PublicationAuthority, PublicationHead, TerminalProposal,
    TerminalReceipt, finish_acquisition,
};

macro_rules! identity {
    ($($name:ident: $doc:literal),* $(,)?) => {$(
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct $name(NonZeroU128);
        impl $name {
            /// Return the opaque identity without changing its namespace.
            #[must_use]
            pub const fn get(self) -> u128 { self.0.get() }
            /// Construct a nonzero service-owned identity; performs no authentication.
            /// # Errors
            /// Returns `ZeroIdentity` for zero.
            pub fn new(value: u128) -> Result<Self, ValueError> {
                NonZeroU128::new(value).map(Self).ok_or(ValueError::ZeroIdentity)
            }
        }
    )*};
}
identity! {
    AcquisitionId: "Immutable acquisition command identity, not a remote exactly-once guarantee.",
    PrincipalId: "Authenticated requester identity, resolved outside this crate.",
    ProviderAccountId: "Data-only provider account identity; never a credential.",
    AudienceId: "Reviewed audience/redistribution scope identity.",
    StorageScopeId: "Reviewed storage-location/security scope identity.",
}

/// Payload-free acquisition rejection; only transient provider failures permit retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AcquisitionError {
    /// Invalid request range, knowledge cutoff or resource ceilings.
    InvalidRequest,
    /// Missing, conflicting, expired, revoked or out-of-scope rights.
    Rights,
    /// Missing, conflicting, expired or incompatible reviewed provider capability.
    Conformance,
    /// Explicit caller cancellation; remote work may already have occurred.
    Cancelled,
    /// Exclusive command deadline elapsed.
    Deadline,
    /// Response identity, bounds, provenance or knowledge contradicts the request.
    Response,
}
impl std::fmt::Display for AcquisitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "acquisition rejected: {self:?}")
    }
}
impl std::error::Error for AcquisitionError {}

/// Borrowed, reviewed catalogs. Authenticity and legal authority are external obligations.
#[derive(Clone, Copy, Debug)]
pub struct AuthorityCatalogs<'a> {
    /// Retained rights revisions, including withdrawals and scope corrections.
    pub rights: &'a ReferenceHistory<RightsRecord>,
    /// Retained provider review revisions, not self-reported declarations.
    pub providers: &'a ReferenceHistory<ProviderReview>,
}
/// Admission result with exact selected authority and bounded retention; no live capability grant.
#[derive(Debug)]
pub struct AuthorizedAcquisition<'a> {
    request: &'a AcquisitionRequest,
    rights: &'a ReferenceVersion<RightsRecord>,
    provider: &'a ReferenceVersion<ProviderReview>,
    delete_by: edgeagent_market_data_domain::UtcInstant,
    checked_at: edgeagent_market_data_domain::UtcInstant,
}
impl<'a> AuthorizedAcquisition<'a> {
    /// Immutable bounded request for the provider adapter.
    #[must_use]
    pub const fn request(&self) -> &'a AcquisitionRequest {
        self.request
    }
    /// Selected reviewed rights and capability versions, for immutable provenance.
    #[must_use]
    pub const fn authority(
        &self,
    ) -> (
        &'a ReferenceVersion<RightsRecord>,
        &'a ReferenceVersion<ProviderReview>,
    ) {
        (self.rights, self.provider)
    }
    /// Exclusive canonical retention deadline; raw data uses this or a shorter deadline.
    #[must_use]
    pub const fn delete_by(&self) -> edgeagent_market_data_domain::UtcInstant {
        self.delete_by
    }
}
