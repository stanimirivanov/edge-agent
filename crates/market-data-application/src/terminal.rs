//! Prepare outside transactions; commit terminal facts through an owner-owned port.
mod native;
mod receipt;
use crate::canonical::canonical;
use crate::*;
use edgeagent_contracts::evidence::*;
pub use receipt::{AcquisitionPublication, TerminalReceipt, finish_acquisition};
use std::fmt;
type Error = BudgetError;
use BudgetErrorKind as K;

/// Trusted local publication policy, distinct from permission to read an existing object.
/// Resolve current rights/provider-review catalogs against the retained admission.
/// Implementations must perform no external I/O inside the publication transaction.
pub trait PublicationAuthority: EvidenceAccess {
    /// Recheck authenticated scope, current authority and unchanged admission pins.
    /// # Errors
    /// Deny unknown, changed, revoked or expired authority; hashes never grant permission.
    fn authorize_publication(
        &self,
        record: &AdmissionRecord,
        object: &EvidenceObject,
    ) -> Result<(), Error>;
}

/// Immutable ready-publication fence; hashes confer no data-access permission.
#[derive(Clone)]
pub struct PublicationHead {
    /// Versioned scoped series identity, independent of acquisition and capture time.
    pub key: EvidenceDigest,
    /// New canonical object identity.
    pub digest: EvidenceDigest,
    /// Exact expected predecessor; None requires a previously unpublished series.
    pub replaces: Option<EvidenceDigest>,
    /// New actual capture time.
    pub captured_at: EvidenceTime,
    /// Inclusive knowledge cutoff; must not regress against the current head.
    pub cutoff: EvidenceTime,
}
impl fmt::Debug for PublicationHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublicationHead").finish_non_exhaustive()
    }
}
/// Sealed preparation result, not evidence of a database commit or present read permission.
#[derive(Clone)]
pub struct TerminalProposal {
    decision: EvidenceDecision,
    head: Option<PublicationHead>,
    observations: Vec<(EvidenceDigest, EvidenceDigest)>,
}
impl fmt::Debug for TerminalProposal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerminalProposal").finish_non_exhaustive()
    }
}
impl TerminalProposal {
    /// Recheck complete admission/quality, freeze and verify immutable storage before publication.
    /// All storage I/O occurs here, outside the publication transaction. Uncertain writes
    /// recover under the same object identity; never delete as compensation.
    /// # Errors
    /// Rejects changed admission/authority, quality, storage integrity, expiry or cancellation.
    pub async fn ready(
        record: &AdmissionRecord,
        admitted: &AuthorizedAcquisition<'_>,
        catalogs: AuthorityCatalogs<'_>,
        cancellation: &Cancellation,
        input: EvidenceFreeze<'_>,
        storage: &impl EvidenceStorage,
        access: &impl PublicationAuthority,
    ) -> Result<Self, Error> {
        record.verify(admitted, input.series.listing, input.policy)?;
        if input.command != record.command() {
            return Err(Error::new(K::Conflict));
        }
        let now = access.now();
        if now.unix_millis() >= record.valid_until_ms()? {
            return Err(Error::new(K::Deadline));
        }
        let mut artifact = freeze_evidence(admitted, catalogs, now, cancellation, input)
            .map_err(|e| Error::with_source(K::Invalid, e))?;
        // Fresh authority can extend its own calculated window, not the retained admission.
        let original = record.delete_by_ms()?;
        let retained = EvidenceTime {
            seconds: original.div_euclid(1000),
            nanos: u32::try_from(original.rem_euclid(1000)).map_err(|_| Error::new(K::Invalid))?
                * 1_000_000,
        };
        if artifact.data().delete_by > retained {
            let mut data = artifact.data().clone();
            data.delete_by = retained;
            artifact = DailyEvidenceArtifact::new(data).map_err(|_| Error::new(K::Invalid))?;
        }
        let decision = EvidenceDecision::ready(record.command().clone(), &artifact)
            .map_err(|_| Error::new(K::Invalid))?;
        if let EvidenceOutcome::Ready { object } = &decision.data().outcome {
            access.authorize_publication(record, object)?;
        }
        let stored = storage
            .put(&artifact, access)
            .await
            .map_err(storage_error)?;
        let EvidenceOutcome::Ready { object } = &decision.data().outcome else {
            return Err(Error::new(K::Invalid));
        };
        if &stored != object {
            return Err(Error::new(K::Invalid));
        }
        if cancellation.is_cancelled() {
            return Err(Error::new(K::Cancelled));
        }
        if access.now().unix_millis() >= record.valid_until_ms()? {
            return Err(Error::new(K::Deadline));
        }
        access.authorize(object).map_err(storage_error)?;
        access.authorize_publication(record, object)?;
        let data = artifact.data();
        // Licensed observation identity excludes capture/request/range and selected reference
        // mappings. Those may evolve without rewriting an immutable source observation.
        let scope = &data.scope;
        let base = (
            &scope.principal,
            &scope.account,
            &scope.source,
            &scope.instrument,
            &scope.audience,
            &scope.storage,
            &scope.endpoint,
            &object.rights.id,
            &data.semantics,
        );
        let hash = |value: &serde_json::Value| -> Result<EvidenceDigest, Error> {
            Ok(DailyEvidenceArtifact::hash(&canonical(value.clone())?))
        };
        let base = serde_json::to_value(base).map_err(|_| Error::new(K::Invalid))?;
        let key = hash(&serde_json::json!([
            "edgeagent.publication-series.v1",
            base,
            scope.listing,
            data.range
        ]))?;
        let mut observations = Vec::with_capacity(data.bars.len());
        for bar in &data.bars {
            let identity = hash(&serde_json::json!([
                "edgeagent.observation.v1",
                base,
                bar.observation
            ]))?;
            let mut fields = serde_json::to_value(bar).map_err(|_| Error::new(K::Invalid))?;
            let fields = fields
                .as_object_mut()
                .ok_or_else(|| Error::new(K::Invalid))?;
            for key in ["mapping", "calendar", "actions", "transformation"] {
                fields.remove(key);
            }
            // BTreeMap makes this stable even when another crate enables preserve_order.
            let bytes = canonical(serde_json::Value::Object(fields.clone()))?;
            observations.push((identity, DailyEvidenceArtifact::hash(&bytes)));
        }
        Ok(Self {
            decision,
            head: Some(PublicationHead {
                key,
                digest: artifact.digest().clone(),
                replaces: data.replaces.clone(),
                captured_at: data.captured_at,
                cutoff: data.cutoff,
            }),
            observations,
        })
    }
    /// Prepare an owner-classified terminal rejection, never a free-form provider error.
    #[must_use]
    pub fn rejected(command: AcquireEvidence, reason: EvidenceRejection) -> Self {
        Self {
            decision: EvidenceDecision::rejected(command, reason),
            head: None,
            observations: Vec::new(),
        }
    }
    /// Reference-only terminal decision, with no licensed prices or provider body.
    #[must_use]
    pub const fn decision(&self) -> &EvidenceDecision {
        &self.decision
    }
    /// Scoped compare-and-swap fence for ready outcomes.
    #[must_use]
    pub const fn head(&self) -> Option<&PublicationHead> {
        self.head.as_ref()
    }
    /// Bounded scoped observation-version identities and immutable content fingerprints.
    #[must_use]
    pub fn observations(&self) -> &[(EvidenceDigest, EvidenceDigest)] {
        &self.observations
    }
    /// Check the last persisted execution state before ready publication.
    /// Rejections retain unresolved charges and permits; they never release them.
    /// # Errors
    /// Rejects mismatched identity, pending/cancelled ready work or expired ready evidence.
    pub fn verify(
        &self,
        record: &AdmissionRecord,
        state: &BudgetSnapshot,
        now: i64,
    ) -> Result<(), Error> {
        if &self.decision.data().request != record.command() {
            return Err(Error::new(K::Conflict));
        }
        if let EvidenceOutcome::Ready { object } = &self.decision.data().outcome {
            if state.cancelled {
                return Err(Error::new(K::Cancelled));
            }
            if state.pending.is_some() {
                return Err(Error::new(K::Busy));
            }
            let delete = i128::from(object.delete_by.seconds) * 1000
                + i128::from(object.delete_by.nanos / 1_000_000);
            if now < record.request().admitted_at.unix_millis()
                || now >= record.valid_until_ms()?
                || i128::from(now) >= delete
            {
                return Err(Error::new(K::Deadline));
            }
        }
        Ok(())
    }
}
pub(super) fn storage_error(e: EvidenceStorageError) -> Error {
    let kind = match e.kind() {
        EvidenceStorageErrorKind::Denied => K::Denied,
        EvidenceStorageErrorKind::Expired | EvidenceStorageErrorKind::Deadline => K::Deadline,
        EvidenceStorageErrorKind::Backend => K::Storage,
        _ => K::Invalid,
    };
    Error::with_source(kind, e)
}
