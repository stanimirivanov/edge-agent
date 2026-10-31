//! Pure caller-scoped Research view. Authenticate owner records outside this reducer.
//! Lifecycle facts append explicit-time owner revisions without changing outcome bytes;
//! callers must not advance consumption progress after a rejected/gapped input.
use crate::{ReplayError as Error, ReplayOutcome};
use edgeagent_contracts::research::{MAX_RESEARCH_BYTES, ResearchRecord, ResearchRequest};
use edgeagent_contracts::research_decision::DecisionProvenance;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
/// Bounded checkpoint, including escaped canonical request/outcome and checksum.
pub const MAX_CHECKPOINT_BYTES: usize = 5 * MAX_RESEARCH_BYTES + 1024;
/// Observed owner outcome, not freshness, license permission or authoritative rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResearchStatus {
    /// No terminal outcome observed for this known request; projection may be behind.
    Pending,
    /// An immutable artifact was observed; expiry/lifecycle must be checked separately.
    Artifact,
    /// An owner-classified terminal abstention was observed.
    Abstained,
    /// Owner invalidation.
    Invalidated,
    /// Explicit owner expiry.
    Expired,
    /// Final owner withdrawal.
    Withdrawn,
    /// Final replacement link.
    Corrected,
}
/// Independent per-request view, never financial/account audit state.
pub struct ResearchProjection {
    request: ResearchRequest,
    request_bytes: Vec<u8>,
    outcome: Option<Vec<u8>>,
    status: ResearchStatus,
    lifecycle: Vec<Vec<u8>>,
    provenance: Option<Vec<u8>>,
}
impl std::fmt::Debug for ResearchProjection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResearchProjection")
            .field("revision", &self.revision())
            .finish_non_exhaustive()
    }
}
impl ResearchProjection {
    /// Start from an independently authenticated request/admission anchor.
    /// # Errors
    /// Rejects invalid or unbounded Research v1 requests.
    pub fn new(request: ResearchRequest) -> Result<Self, Error> {
        let record = ResearchRecord::Request(request);
        let request_bytes = record.canonical_bytes().map_err(|_| Error::Invalid)?;
        let ResearchRecord::Request(request) = record else {
            return Err(Error::Invalid);
        };
        Ok(Self {
            request,
            request_bytes,
            outcome: None,
            status: ResearchStatus::Pending,
            lifecycle: Vec::new(),
            provenance: None,
        })
    }
    /// Immutable caller/request/policy anchor; it confers no owner authenticity.
    #[must_use]
    pub const fn request(&self) -> &ResearchRequest {
        &self.request
    }
    /// Observed status; Pending is not evidence that an owner rejected the request.
    #[must_use]
    pub const fn status(&self) -> ResearchStatus {
        self.status
    }
    /// Local revision: zero pending, one terminal, then one per owner lifecycle fact and observed provenance.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        if matches!(self.status, ResearchStatus::Pending) {
            0
        } else {
            1 + self.lifecycle.len() as u64 + self.provenance.is_some() as u64
        }
    }
    /// Exact canonical immutable terminal bytes, without a new freshness/entitlement claim.
    #[must_use]
    pub fn outcome_bytes(&self) -> Option<&[u8]> {
        self.outcome.as_deref()
    }
    /// Exact authenticated companion bytes, or explicit absence for legacy/pending views.
    /// Absence must never be interpreted as a successful validation or policy approval.
    #[must_use]
    pub fn provenance_bytes(&self) -> Option<&[u8]> {
        self.provenance.as_deref()
    }
    /// Apply separately authenticated owner provenance. Missing outcome returns Gap.
    /// All refusal paths leave outcome, lifecycle, checkpoint and progress unchanged.
    pub fn apply_provenance(&mut self, record: DecisionProvenance) -> Result<ReplayOutcome, Error> {
        let bytes = record.canonical_bytes().map_err(|_| Error::Invalid)?;
        self.bind(&record.request)?;
        let outcome = self.outcome.as_ref().ok_or(Error::Gap)?;
        let outcome = ResearchRecord::decode(outcome).map_err(|_| Error::Invalid)?;
        record.bind(&outcome).map_err(|_| Error::Conflict)?;
        if let Some(prior) = &self.provenance {
            return if prior == &bytes {
                Ok(ReplayOutcome::Duplicate)
            } else {
                Err(Error::Conflict)
            };
        }
        self.provenance = Some(bytes);
        Ok(ReplayOutcome::Applied)
    }
    fn bind(&self, request: &ResearchRequest) -> Result<(), Error> {
        if (request.scope.as_str(), request.request_id.as_str())
            != (
                self.request.scope.as_str(),
                self.request.request_id.as_str(),
            )
        {
            return Err(Error::Scope);
        }
        if request != &self.request {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    /// Apply an authenticated owner record; all errors leave state/checkpoint unchanged.
    /// # Errors
    /// Rejects malformed, foreign or conflicting facts. Missing lifecycle revisions return Gap,
    /// requiring backfill/handler support; they are never consumed or silently skipped.
    pub fn apply(&mut self, record: ResearchRecord) -> Result<ReplayOutcome, Error> {
        let bytes = record.canonical_bytes().map_err(|_| Error::Invalid)?;
        let status = match &record {
            ResearchRecord::Request(request) => {
                self.bind(request)?;
                return Ok(ReplayOutcome::Duplicate);
            }
            ResearchRecord::Artifact(artifact) => {
                self.bind(&artifact.request)?;
                ResearchStatus::Artifact
            }
            ResearchRecord::Abstention(abstention) => {
                self.bind(&abstention.request)?;
                ResearchStatus::Abstained
            }
            ResearchRecord::Lifecycle(change) => {
                return self.apply_lifecycle(change.clone(), bytes);
            }
        };
        if let Some(outcome) = &self.outcome {
            return if outcome == &bytes {
                Ok(ReplayOutcome::Duplicate)
            } else {
                Err(Error::Conflict)
            };
        }
        self.outcome = Some(bytes);
        self.status = status;
        Ok(ReplayOutcome::Applied)
    }
    /// Exact retained lifecycle records, in consecutive owner order (at most two).
    #[must_use]
    pub fn lifecycle_records(&self) -> &[Vec<u8>] {
        &self.lifecycle
    }
    fn apply_lifecycle(
        &mut self,
        change: edgeagent_contracts::research::ResearchLifecycle,
        bytes: Vec<u8>,
    ) -> Result<ReplayOutcome, Error> {
        use edgeagent_contracts::research::LifecycleChange;
        use edgeagent_research_domain::lifecycle::{Head, State};
        if change.scope != self.request.scope || change.request_id != self.request.request_id {
            return Err(Error::Scope);
        }
        let Some(outcome) = &self.outcome else {
            return Err(Error::Gap);
        };
        let ResearchRecord::Artifact(artifact) =
            ResearchRecord::decode(outcome).map_err(|_| Error::Invalid)?
        else {
            return Err(Error::Conflict);
        };
        if ResearchRecord::Artifact(artifact.clone())
            .digest()
            .map_err(|_| Error::Invalid)?
            != change.artifact
        {
            return Err(Error::Conflict);
        }
        if change.revision <= self.lifecycle.len() as u64 {
            return if self.lifecycle.get(change.revision as usize - 1) == Some(&bytes) {
                Ok(ReplayOutcome::Duplicate)
            } else {
                Err(Error::Conflict)
            };
        }
        if change.revision != self.lifecycle.len() as u64 + 1 {
            return Err(Error::Gap);
        }
        let state = match change.change {
            LifecycleChange::Invalidated => State::Invalidated,
            LifecycleChange::Expired => State::Expired,
            LifecycleChange::Withdrawn => State::Withdrawn,
            LifecycleChange::Corrected { .. } => State::Corrected,
        };
        let (prior, time) = match self.lifecycle.last() {
            Some(bytes) => {
                let ResearchRecord::Lifecycle(last) =
                    ResearchRecord::decode(bytes).map_err(|_| Error::Invalid)?
                else {
                    return Err(Error::Invalid);
                };
                let prior = match last.change {
                    LifecycleChange::Invalidated => State::Invalidated,
                    LifecycleChange::Expired => State::Expired,
                    LifecycleChange::Withdrawn => State::Withdrawn,
                    LifecycleChange::Corrected { .. } => State::Corrected,
                };
                (prior, last.effective_at_ms)
            }
            None => (State::Published, artifact.request.decision_at_ms),
        };
        Head {
            state: prior,
            revision: self.lifecycle.len() as u64,
            effective_at_ms: time,
        }
        .transition(
            change.revision,
            change.effective_at_ms,
            artifact.expires_at_ms,
            state,
        )
        .map_err(|_| Error::Conflict)?;
        self.status = match state {
            State::Published => ResearchStatus::Artifact,
            State::Invalidated => ResearchStatus::Invalidated,
            State::Expired => ResearchStatus::Expired,
            State::Withdrawn => ResearchStatus::Withdrawn,
            State::Corrected => ResearchStatus::Corrected,
        };
        self.lifecycle.push(bytes);
        Ok(ReplayOutcome::Applied)
    }
    /// Encode a complete disposable view. Persist atomically with consumer progress.
    /// Checksum detects corruption, not authenticity or an authorized storage rollback.
    /// # Errors
    /// Rejects encoding/capacity failures.
    pub fn checkpoint(&self) -> Result<Vec<u8>, Error> {
        let checkpoint = Checkpoint {
            version: if self.provenance.is_some() {
                "EARP3"
            } else if self.lifecycle.is_empty() {
                "EARP1"
            } else {
                "EARP2"
            }
            .into(),
            lifecycle: self
                .lifecycle
                .iter()
                .map(|b| String::from_utf8(b.clone()).map_err(|_| Error::Invalid))
                .collect::<Result<_, _>>()?,
            provenance: self
                .provenance
                .as_ref()
                .map(|v| String::from_utf8(v.clone()))
                .transpose()
                .map_err(|_| Error::Invalid)?,
            request: String::from_utf8(self.request_bytes.clone()).map_err(|_| Error::Invalid)?,
            outcome: self
                .outcome
                .as_ref()
                .map(|v| String::from_utf8(v.clone()))
                .transpose()
                .map_err(|_| Error::Invalid)?,
        };
        let mut bytes = serde_json::to_vec(&checkpoint).map_err(|_| Error::Invalid)?;
        bytes.extend(Sha256::digest(&bytes));
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(Error::Capacity);
        }
        Ok(bytes)
    }
    /// Restore against the independently authenticated original request. Storage trust
    /// and rollback fencing remain external, as with the account projection checkpoint.
    /// # Errors
    /// Rejects bad version/checksum, altered anchor, noncanonical encoding or invalid outcome.
    pub fn restore(request: ResearchRequest, bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(Error::Capacity);
        }
        if bytes.len() < 32 {
            return Err(Error::Invalid);
        }
        let (body, checksum) = bytes.split_at(bytes.len() - 32);
        if Sha256::digest(body).as_slice() != checksum {
            return Err(Error::Invalid);
        }
        let checkpoint: Checkpoint = serde_json::from_slice(body).map_err(|_| Error::Invalid)?;
        if !matches!(checkpoint.version.as_str(), "EARP1" | "EARP2" | "EARP3") {
            return Err(Error::Invalid);
        }
        let mut view = Self::new(request)?;
        if checkpoint.request.as_bytes() != view.request_bytes {
            return Err(Error::Scope);
        }
        if let Some(outcome) = checkpoint.outcome {
            let record = ResearchRecord::decode(outcome.as_bytes()).map_err(|_| Error::Invalid)?;
            if !matches!(
                record,
                ResearchRecord::Artifact(_) | ResearchRecord::Abstention(_)
            ) {
                return Err(Error::Invalid);
            }
            view.apply(record)?;
        }
        if checkpoint.lifecycle.len() > 2 {
            return Err(Error::Capacity);
        }
        for change in checkpoint.lifecycle {
            view.apply(ResearchRecord::decode(change.as_bytes()).map_err(|_| Error::Invalid)?)?;
        }
        if let Some(provenance) = checkpoint.provenance {
            view.apply_provenance(
                DecisionProvenance::decode(provenance.as_bytes()).map_err(|_| Error::Invalid)?,
            )?;
        }
        if view.checkpoint() != Ok(bytes.to_vec()) {
            return Err(Error::Invalid);
        }
        Ok(view)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    version: String,
    request: String,
    outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    lifecycle: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provenance: Option<String>,
}
