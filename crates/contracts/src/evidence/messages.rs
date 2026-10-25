use super::*;
use crate::{Component, EventRetention, MessageDefinition, MessageRegistry, MessageRoutingError};
use serde::{Deserialize, Serialize};

/// MarketData-owned acquisition command and one terminal authoritative outcome.
pub const DEFINITIONS: [MessageDefinition; 2] = [
    MessageDefinition::command(
        "com.edgeagent.market-data.acquire-evidence.v1",
        "urn:edgeagent:schema:market-data:acquire-evidence:v1",
        Component::MarketData,
        "acquisition",
    ),
    MessageDefinition::event(
        "com.edgeagent.market-data.evidence-acquisition-decided.v1",
        "urn:edgeagent:schema:market-data:evidence-acquisition-decided:v1",
        Component::MarketData,
        "acquisition",
        EventRetention::Audit,
    ),
];
/// Exact registry; deployment composition decides when to provision/activate it.
/// # Errors
/// Rejects an invalid compiled-in routing definition.
pub fn registry() -> Result<MessageRegistry<'static>, MessageRoutingError> {
    MessageRegistry::new(&DEFINITIONS)
}
/// Identity-only request for an owner-retained immutable admission record; no actor, URL or price.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AcquireEvidence {
    /// Immutable command identity, also the acquisition partition key.
    pub request: EvidenceId,
    /// Digest binding the complete owner-admitted request, not an API capability.
    pub request_sha256: EvidenceDigest,
}
impl AcquireEvidence {
    /// Decode only the exact acquisition route and require payload/partition identity agreement.
    /// # Errors
    /// Rejects schema/type/partition mismatches and invalid typed payloads.
    pub fn from_envelope(envelope: &crate::MessageEnvelope) -> Result<Self, EvidenceContractError> {
        DEFINITIONS[0]
            .validate_envelope(envelope)
            .map_err(|_| EvidenceContractError)?;
        let request: Self = envelope.payload().map_err(|_| EvidenceContractError)?;
        if envelope.partition_key()
            != Some(format!("acquisition/{}", request.request.as_str()).as_str())
        {
            return Err(EvidenceContractError);
        }
        Ok(request)
    }
}
/// Scoped object identity. Hashes/locations are not permission to read it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvidenceObject {
    /// Canonical object digest.
    pub sha256: EvidenceDigest,
    /// Exact encoded length, within the 4 MiB object limit.
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 4194304)))]
    pub bytes: u32,
    /// Storage scope; the adapter must also bind rights and audience.
    pub storage: EvidenceId,
    /// Licensed audience, not a bearer token.
    pub audience: EvidenceId,
    /// Exact selected rights version.
    pub rights: EvidenceVersion,
    /// Exclusive deletion/use deadline.
    pub delete_by: EvidenceTime,
    /// Linked prior immutable snapshot, null for an original capture.
    #[serde(deserialize_with = "super::model::required")]
    pub replaces: Option<EvidenceDigest>,
}
/// Stable payload-free terminal failure reason; details stay owner-private.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum EvidenceRejection {
    /// Current rights unavailable.
    Rights,
    /// Provider contract not qualified.
    Conformance,
    /// Whole-series quality failed.
    Quality,
    /// Caller cancelled.
    Cancelled,
    /// Command deadline elapsed.
    Deadline,
    /// Durable resource budget exhausted.
    Budget,
    /// Provider failed terminally.
    Provider,
}
/// Terminal outcome; a rejection cannot carry a usable object or synthetic prices.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum EvidenceOutcome {
    /// Only publish after immutable storage and owner commit.
    Ready {
        /// Scoped canonical object identity.
        object: EvidenceObject,
    },
    /// Terminal no-evidence result.
    Rejected {
        /// Bounded reason, never provider text.
        reason: EvidenceRejection,
    },
}
/// Untrusted terminal event input. The validated wrapper prevents invalid object bounds/times.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EvidenceDecisionData {
    /// Exact acquisition request binding.
    pub request: AcquireEvidence,
    /// Mutually exclusive ready/rejected outcome.
    pub outcome: EvidenceOutcome,
}
/// Validated terminal event; this contract alone does not prove storage or transaction commit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "EvidenceDecisionData", into = "EvidenceDecisionData")]
pub struct EvidenceDecision(EvidenceDecisionData);
impl TryFrom<EvidenceDecisionData> for EvidenceDecision {
    type Error = EvidenceContractError;
    fn try_from(data: EvidenceDecisionData) -> Result<Self, Self::Error> {
        if let EvidenceOutcome::Ready { object } = &data.outcome
            && (object.bytes == 0
                || object.bytes as usize > MAX_EVIDENCE_BYTES
                || !object.delete_by.valid()
                || object.delete_by.nanos % 1_000_000 != 0
                || object.replaces.as_ref() == Some(&object.sha256))
        {
            return Err(EvidenceContractError);
        }
        Ok(Self(data))
    }
}
impl From<EvidenceDecision> for EvidenceDecisionData {
    fn from(value: EvidenceDecision) -> Self {
        value.0
    }
}
impl EvidenceDecision {
    /// Bind a native accepted artifact to the unchanged reference-only terminal contract.
    /// Consumers must dispatch the retrieved object's explicit format; raw captures are ineligible.
    /// # Errors
    /// Rejects a request inconsistent with the accepted artifact.
    pub fn ready_native(
        request: AcquireEvidence,
        artifact: &NativeDailyArtifact,
    ) -> Result<Self, EvidenceContractError> {
        let scope = &artifact.data().capture.scope;
        if request.request != scope.request || request.request_sha256 != scope.request_sha256 {
            return Err(EvidenceContractError);
        }
        Self::try_from(EvidenceDecisionData {
            request,
            outcome: EvidenceOutcome::Ready {
                object: artifact.object()?,
            },
        })
    }
    /// Decode only authoritative MarketData outcomes on the matching acquisition partition.
    /// # Errors
    /// Rejects wrong producer/type/schema/partition or invalid terminal data.
    pub fn from_envelope(envelope: &crate::MessageEnvelope) -> Result<Self, EvidenceContractError> {
        DEFINITIONS[1]
            .validate_envelope(envelope)
            .map_err(|_| EvidenceContractError)?;
        let decision: Self = envelope.payload().map_err(|_| EvidenceContractError)?;
        if envelope.partition_key()
            != Some(format!("acquisition/{}", decision.0.request.request.as_str()).as_str())
        {
            return Err(EvidenceContractError);
        }
        Ok(decision)
    }
    /// Bind reference metadata to the actual canonical object; this performs no storage/publication.
    /// # Errors
    /// Rejects a request binding inconsistent with the artifact.
    pub fn ready(
        request: AcquireEvidence,
        artifact: &DailyEvidenceArtifact,
    ) -> Result<Self, EvidenceContractError> {
        let d = artifact.data();
        if request.request != d.scope.request || request.request_sha256 != d.scope.request_sha256 {
            return Err(EvidenceContractError);
        }
        let rights = d
            .references
            .iter()
            .find(|r| r.kind == EvidenceReferenceKind::Rights)
            .ok_or(EvidenceContractError)?
            .version
            .clone();
        Self::try_from(EvidenceDecisionData {
            request,
            outcome: EvidenceOutcome::Ready {
                object: EvidenceObject {
                    sha256: artifact.digest().clone(),
                    bytes: u32::try_from(artifact.bytes().len())
                        .map_err(|_| EvidenceContractError)?,
                    storage: d.scope.storage.clone(),
                    audience: d.scope.audience.clone(),
                    rights,
                    delete_by: d.delete_by,
                    replaces: d.replaces.clone(),
                },
            },
        })
    }
    /// Construct a terminal rejection, without a reference, price or free-form text.
    #[must_use]
    pub fn rejected(request: AcquireEvidence, reason: EvidenceRejection) -> Self {
        Self(EvidenceDecisionData {
            request,
            outcome: EvidenceOutcome::Rejected { reason },
        })
    }
    /// Immutable validated event data.
    #[must_use]
    pub const fn data(&self) -> &EvidenceDecisionData {
        &self.0
    }
}

impl NativeDailyArtifact {
    /// Exact scoped descriptor; neither the descriptor nor its digest grants access.
    /// # Errors
    /// Rejects an internally missing rights witness or invalid encoded length.
    pub fn object(&self) -> Result<EvidenceObject, EvidenceContractError> {
        let data = self.data();
        let rights = data
            .references
            .iter()
            .find(|r| r.kind == EvidenceReferenceKind::Rights)
            .ok_or(EvidenceContractError)?;
        Ok(EvidenceObject {
            sha256: self.digest().clone(),
            bytes: u32::try_from(self.bytes().len()).map_err(|_| EvidenceContractError)?,
            storage: data.capture.scope.storage.clone(),
            audience: data.capture.scope.audience.clone(),
            rights: rights.version.clone(),
            delete_by: data.delete_by,
            replaces: data.replaces.clone(),
        })
    }
}
