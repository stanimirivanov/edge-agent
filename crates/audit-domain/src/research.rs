//! Pure caller-scoped Research view. Authenticate owner records outside this reducer.
//! Lifecycle facts are explicit gaps until their separate transition implementation;
//! callers must not advance consumption progress after a rejected/gapped input.
use crate::{ReplayError as Error, ReplayOutcome};
use edgeagent_contracts::research::{MAX_RESEARCH_BYTES, ResearchRecord, ResearchRequest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
/// Bounded checkpoint, including escaped canonical request/outcome and checksum.
pub const MAX_CHECKPOINT_BYTES: usize = 4 * MAX_RESEARCH_BYTES + 1024;
/// Observed owner outcome, not freshness, license permission or authoritative rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResearchStatus {
    /// No terminal outcome observed for this known request; projection may be behind.
    Pending,
    /// An immutable artifact was observed; expiry/lifecycle must be checked separately.
    Artifact,
    /// An owner-classified terminal abstention was observed.
    Abstained,
}
/// Independent per-request view, never financial/account audit state.
pub struct ResearchProjection {
    request: ResearchRequest,
    request_bytes: Vec<u8>,
    outcome: Option<Vec<u8>>,
    status: ResearchStatus,
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
    /// Local view revision: zero pending, one terminal. Never an invented owner sequence.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        if matches!(self.status, ResearchStatus::Pending) {
            0
        } else {
            1
        }
    }
    /// Exact canonical immutable terminal bytes, without a new freshness/entitlement claim.
    #[must_use]
    pub fn outcome_bytes(&self) -> Option<&[u8]> {
        self.outcome.as_deref()
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
    /// Rejects malformed, foreign or conflicting facts. Lifecycle facts return Gap,
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
                if change.scope != self.request.scope
                    || change.request_id != self.request.request_id
                {
                    return Err(Error::Scope);
                }
                if let Some(outcome) = &self.outcome {
                    if self.status != ResearchStatus::Artifact {
                        return Err(Error::Conflict);
                    }
                    let digest = ResearchRecord::decode(outcome)
                        .map_err(|_| Error::Invalid)?
                        .digest()
                        .map_err(|_| Error::Invalid)?;
                    if digest != change.artifact {
                        return Err(Error::Conflict);
                    }
                }
                return Err(Error::Gap);
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
    /// Encode a complete disposable view. Persist atomically with consumer progress.
    /// Checksum detects corruption, not authenticity or an authorized storage rollback.
    /// # Errors
    /// Rejects encoding/capacity failures.
    pub fn checkpoint(&self) -> Result<Vec<u8>, Error> {
        let checkpoint = Checkpoint {
            version: "EARP1".into(),
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
        if checkpoint.version != "EARP1" {
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
}
