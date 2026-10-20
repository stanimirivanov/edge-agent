use super::{OrderError, OrderEventId, OrderId, ProposedFill};
use crate::ArtifactDigest;
use edgeagent_market_data_domain::{PreciseInstant, SemanticReference};

/// Rejection category assigned by P08's authoritative admission policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OrderRejection {
    /// Scenario, product or account authorization did not qualify.
    Ineligible,
    /// Aggregate/strategy risk rejected the request.
    Risk,
    /// Cash or long inventory could not be reserved.
    Funding,
    /// Required qualified market evidence is unavailable.
    Evidence,
}
/// Fill proposal plus pinned evidence/model facts. No ledger or fee calculation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FillFact {
    pub(super) proposal: ProposedFill,
    pub(super) model: (SemanticReference, ArtifactDigest),
    pub(super) evidence: ArtifactDigest,
}
impl FillFact {
    /// Bind a validated proposal to retained configuration and evidence artifacts.
    /// These identifiers are claims, not independent quality certification.
    #[must_use]
    pub const fn new(
        proposal: ProposedFill,
        model: (SemanticReference, ArtifactDigest),
        evidence: ArtifactDigest,
    ) -> Self {
        Self {
            proposal,
            model,
            evidence,
        }
    }
    /// Proposal, model identity/configuration and evidence artifact digest.
    #[must_use]
    pub const fn evidence(
        self,
    ) -> (
        ProposedFill,
        (SemanticReference, ArtifactDigest),
        ArtifactDigest,
    ) {
        (self.proposal, self.model, self.evidence)
    }
}
/// Append-only lifecycle effects. They become authoritative only after P08 commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OrderEventKind {
    /// Admission succeeded; this aggregate alone cannot establish funding/risk.
    Accepted,
    /// Admission failed before acceptance.
    Rejected(OrderRejection),
    /// Cancellation is pending; do not release reservations.
    CancelRequested,
    /// Pending cancellation failed; resume the prior working/partial state.
    CancelRejected,
    /// Remaining quantity was definitively cancelled; retain historical fills.
    Cancelled,
    /// Explicit deadline passed; retain historical fills.
    Expired,
    /// One incremental fill, not a cumulative quantity or a live confirmation.
    Filled(FillFact),
    /// Retained non-fill decision; replay must not resample this attempt.
    NoFill {
        /// Pinned model revision and configuration digest.
        model: (SemanticReference, ArtifactDigest),
        /// Retained evidence artifact digest, not proof of qualification.
        evidence: ArtifactDigest,
    },
}
/// Immutable fact identity, contiguous order revision, virtual time and effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderEvent {
    pub(super) id: OrderEventId,
    pub(super) order: OrderId,
    pub(super) revision: u64,
    pub(super) at: PreciseInstant,
    pub(super) kind: OrderEventKind,
}
impl OrderEvent {
    /// Construct a fact candidate. Application still validates order state/history.
    ///
    /// # Errors
    /// Returns `Revision` for zero; aggregate application rejects gaps/conflicts.
    pub fn new(
        id: OrderEventId,
        order: OrderId,
        revision: u64,
        at: PreciseInstant,
        kind: OrderEventKind,
    ) -> Result<Self, OrderError> {
        if revision == 0 {
            return Err(OrderError::Revision);
        }
        Ok(Self {
            id,
            order,
            revision,
            at,
            kind,
        })
    }
    /// Stable event ID, owning order and order revision.
    #[must_use]
    pub const fn position(self) -> (OrderEventId, OrderId, u64) {
        (self.id, self.order, self.revision)
    }
    /// Virtual occurrence time and lifecycle effect.
    #[must_use]
    pub const fn effect(self) -> (PreciseInstant, OrderEventKind) {
        (self.at, self.kind)
    }
}
