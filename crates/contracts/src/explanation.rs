//! Additive explanation wire contracts. Binding checks consistency, not authority.
use crate::{
    Component, EventRetention, MessageDefinition,
    messaging::canonical_json,
    research::{ResearchRecord, digest as research_digest, token},
    research_decision::{Assessment, DecisionProvenance, Reason},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Maximum canonical size per record, checked before decoding.
pub const MAX_BYTES: usize = 16_384;
/// At most 64 ranks, eight policy reasons and one required disclosure.
pub const MAX_CLAIMS: usize = 73;

/// Maximum UTF-8 bytes in a single reviewed excerpt.
pub const MAX_DOCUMENT_TEXT_BYTES: usize = 4096;

/// Route definition for authorized, renderable companions. Refusals stay owner-local.
pub const RECORDED: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.research.explanation-recorded.v1",
    "urn:edgeagent:schema:research:explanation:companion:v1",
    Component::Research,
    "research-request",
    EventRetention::Workflow,
);

/// Payload-safe contract refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid research explanation contract")
    }
}
impl std::error::Error for Error {}

fn digest(value: &str) -> bool {
    research_digest(value) && value.bytes().any(|b| b != b'0')
}

/// Exact scope and immutable Research outcome/decision identities.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub scope: String,
    pub artifact: String,
    pub decision: String,
}
impl Anchor {
    /// Check the shape of all three identity fields.
    pub fn is_valid(&self) -> bool {
        token(&self.scope) && digest(&self.artifact) && digest(&self.decision)
    }

    fn bind(&self, artifact: &ResearchRecord, decision: &DecisionProvenance) -> Result<(), Error> {
        let ResearchRecord::Artifact(value) = artifact else {
            return Err(Error);
        };
        decision.bind(artifact).map_err(|_| Error)?;
        if self.scope != value.request.scope
            || self.artifact != artifact.digest().map_err(|_| Error)?
            || self.decision != decision.digest().map_err(|_| Error)?
        {
            return Err(Error);
        }
        Ok(())
    }
}

/// Immutable explanation inputs selected by Research, outside model control.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub anchor: Anchor,
    pub profile: String,
    pub evidence: String,
}

impl Binding {
    /// Check the shape of the anchor and immutable input identities.
    pub fn is_valid(&self) -> bool {
        self.anchor.is_valid() && digest(&self.profile) && digest(&self.evidence)
    }
}

/// One scheduling intent per published artifact and decision.
/// Explanation profile changes do not create a second job identity.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub anchor: Anchor,
}

/// Required limit on what an explanation authorizes.
#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Disclosure {
    ResearchOnly,
}

/// Claim identity only. Numeric values, policy decisions and prose are renderer inputs.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "claim", rename_all = "snake_case", deny_unknown_fields)]
pub enum Claim {
    Ranked { instrument: String },
    PolicyReason { reason: Reason },
    Disclosure { disclosure: Disclosure },
}

/// Candidate presentation variants. Later validation checks the approved variant
/// for the particular claim and profile.
#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    Standard,
    PlainLanguage,
}

/// Citation identifies one entry in the bound evidence allowlist.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub claim: Claim,
    pub citation: String,
    pub variant: Variant,
}

/// Model proposal. Its containing request must independently supply the binding.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    #[cfg_attr(feature = "schema", schemars(length(min = 1, max = 73)))]
    pub claims: Vec<Selection>,
}
impl Proposal {
    fn valid(&self) -> bool {
        (1..=MAX_CLAIMS).contains(&self.claims.len())
            && self.claims.iter().enumerate().all(|(i, value)| {
                digest(&value.citation)
                    && match &value.claim {
                        Claim::Ranked { instrument } => token(instrument),
                        _ => true,
                    }
                    && !self.claims[..i]
                        .iter()
                        .any(|prior| prior.claim == value.claim)
            })
    }
}

/// Fixed fallback categories for deterministic rendering.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Fallback {
    Disabled,
    Unavailable,
    Timeout,
    BudgetExhausted,
    InvalidProposal,
    UncertainAttempt,
}

/// Refusal carries no renderable claims.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    DisclosureDenied,
    AuthorityUnavailable,
}

/// Retained terminal choice, distinct from the original publication.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Generated {
        proposal: Proposal,
    },
    Fallback {
        reason: Fallback,
        proposal: Proposal,
    },
    Refused {
        reason: Refusal,
    },
}

/// Terminal companion. Generated and fallback variants retain typed selections.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Companion {
    pub binding: Binding,
    pub outcome: Outcome,
}

/// An explicitly supplied excerpt. Its source is an opaque label, and its
/// text is untrusted data even after its identity and rights are checked.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DocumentExcerpt {
    pub source: String,
    pub scope: String,
    pub available_at_ms: i64,
    pub text: String,
}

impl DocumentExcerpt {
    fn valid(&self) -> bool {
        token(&self.source)
            && !matches!(self.source.as_str(), "." | "..")
            && token(&self.scope)
            && self.available_at_ms >= 0
            && (1..=MAX_DOCUMENT_TEXT_BYTES).contains(&self.text.len())
            && !self
                .text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    }

    /// Identity of the exact canonical source, scope, availability and text.
    pub fn id(&self) -> Result<String, Error> {
        Ok(identity(&self.canonical_bytes()?))
    }
}

macro_rules! wire {
    ($type:ident, $wire:ident, $kind:literal, $schema:literal, $valid:expr) => {
        #[derive(Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        #[serde(tag = "kind", content = "data", deny_unknown_fields)]
        enum $wire<T> {
            #[serde(rename = $kind)]
            V1(T),
        }
        impl $type {
            /// Validate and encode exact key-sorted JSON with one final LF.
            pub fn canonical_bytes(&self) -> Result<Vec<u8>, Error> {
                if !$valid(self) {
                    return Err(Error);
                }
                let mut bytes = canonical_json::to_vec(&$wire::V1(self)).map_err(|_| Error)?;
                bytes.push(b'\n');
                if bytes.len() > MAX_BYTES {
                    return Err(Error);
                }
                Ok(bytes)
            }

            /// Decode a bounded, strict canonical v1 record.
            pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
                if bytes.len() > MAX_BYTES {
                    return Err(Error);
                }
                let $wire::V1(value): $wire<Self> =
                    serde_json::from_slice(bytes).map_err(|_| Error)?;
                if value.canonical_bytes()? != bytes {
                    return Err(Error);
                }
                Ok(value)
            }

            /// Structural schema. Canonical and binding checks still apply.
            #[cfg(feature = "schema")]
            pub fn schema() -> Result<Vec<u8>, Error> {
                let schema = schemars::generate::SchemaSettings::draft2020_12()
                    .into_generator()
                    .into_root_schema_for::<$wire<Self>>();
                let mut value = serde_json::to_value(schema).map_err(|_| Error)?;
                value["$id"] = $schema.into();
                let mut bytes = canonical_json::to_vec(&value).map_err(|_| Error)?;
                bytes.push(b'\n');
                Ok(bytes)
            }
        }
    };
}

wire!(
    Proposal,
    ProposalWire,
    "research.explanation.proposal.v1",
    "urn:edgeagent:schema:research:explanation:proposal:v1",
    Proposal::valid
);
wire!(
    Job,
    JobWire,
    "research.explanation.job.v1",
    "urn:edgeagent:schema:research:explanation:job:v1",
    |v: &Job| v.anchor.is_valid()
);
wire!(
    Companion,
    CompanionWire,
    "research.explanation.companion.v1",
    "urn:edgeagent:schema:research:explanation:companion:v1",
    Companion::valid
);

wire!(
    DocumentExcerpt,
    DocumentWire,
    "research.explanation.document.v1",
    "urn:edgeagent:schema:research:explanation:document:v1",
    DocumentExcerpt::valid
);

fn identity(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Job {
    /// Stable job identity from canonical bytes, independent of delivery metadata.
    pub fn id(&self) -> Result<String, Error> {
        Ok(identity(&self.canonical_bytes()?))
    }

    /// Require exact binding to an artifact and its matching decision.
    pub fn bind(
        &self,
        artifact: &ResearchRecord,
        decision: &DecisionProvenance,
    ) -> Result<(), Error> {
        self.canonical_bytes()?;
        self.anchor.bind(artifact, decision)
    }
}

impl Companion {
    fn valid(&self) -> bool {
        self.binding.is_valid()
            && match &self.outcome {
                Outcome::Generated { proposal } => proposal.valid(),
                Outcome::Fallback { proposal, .. } => {
                    proposal.valid()
                        && proposal
                            .claims
                            .iter()
                            .all(|s| s.variant == Variant::Standard)
                }
                Outcome::Refused { .. } => true,
            }
    }

    /// Stable terminal record identity. Use as the event ID only for renderable outcomes.
    pub fn id(&self) -> Result<String, Error> {
        Ok(identity(&self.canonical_bytes()?))
    }

    /// Recover the scheduling intent for this terminal record.
    pub fn job(&self) -> Job {
        Job {
            anchor: self.binding.anchor.clone(),
        }
    }

    /// Check immutable inputs and mandatory coverage/order. Citation membership,
    /// factual rendering and disclosure authority are checked by Research separately.
    pub fn bind(
        &self,
        artifact: &ResearchRecord,
        decision: &DecisionProvenance,
    ) -> Result<(), Error> {
        self.canonical_bytes()?;
        self.binding.anchor.bind(artifact, decision)?;
        let proposal = match &self.outcome {
            Outcome::Generated { proposal } | Outcome::Fallback { proposal, .. } => proposal,
            Outcome::Refused { .. } => return Ok(()),
        };
        let (ResearchRecord::Artifact(value), Assessment::Evaluated { reasons, .. }) =
            (artifact, &decision.assessment)
        else {
            return Err(Error);
        };
        let expected = value
            .ranking
            .iter()
            .map(|rank| Claim::Ranked {
                instrument: rank.instrument.clone(),
            })
            .chain(
                reasons
                    .iter()
                    .map(|reason| Claim::PolicyReason { reason: *reason }),
            )
            .chain([Claim::Disclosure {
                disclosure: Disclosure::ResearchOnly,
            }]);
        if proposal.claims.iter().map(|s| s.claim.clone()).eq(expected) {
            Ok(())
        } else {
            Err(Error)
        }
    }
}
