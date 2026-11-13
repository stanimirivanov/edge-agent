//! Canonical owner-reviewed qualification inputs. Shape alone grants no authority.
use crate::{
    explanation_profile::Profile,
    messaging::canonical_json,
    research::{digest, token},
};
use serde::{Deserialize, Serialize};
pub const MAX_COMMAND_BYTES: usize = 2_097_152;
#[derive(Debug)]
pub struct Error;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid explanation qualification")
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    OwnedLocalRuntimeV1,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "outcome",
    content = "proposal",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Observation {
    Generated(String),
    Fallback,
    Failed,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub input: String,
    pub candidate_first: bool,
    pub candidate: Observation,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preference {
    First,
    Second,
    Tie,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rating {
    pub id: String,
    pub presentation: String,
    pub preference: Preference,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub reviewer: String,
    pub ratings: Vec<Rating>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    pub profile: Profile,
    pub suite: String,
    pub preparation: String,
    /// Exact private manifest and rebuilt helper executable identities.
    pub manifest: String,
    pub helper: String,
    pub provenance: Provenance,
    pub cases: Vec<Case>,
    pub safety: Vec<Case>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub collection: Collection,
    pub reviews: [Review; 2],
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Approve {
        scope: String,
        revision: i64,
        attestation: String,
        evidence: Box<Evidence>,
    },
    ApproveRuntime {
        scope: String,
        revision: i64,
        attestation: String,
        evidence: Box<Evidence>,
    },
    Revoke {
        scope: String,
        revision: i64,
        attestation: String,
        profile: Profile,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
enum Wire<T> {
    #[serde(rename = "research.explanation-qualification.review.v1")]
    V1(T),
    #[serde(rename = "research.explanation-qualification.review.v2")]
    V2(T),
}
fn pin(value: &str) -> bool {
    digest(value) && value.bytes().any(|b| b != b'0')
}
impl Command {
    pub fn scope(&self) -> &str {
        match self {
            Self::ApproveRuntime { scope, .. }
            | Self::Approve { scope, .. }
            | Self::Revoke { scope, .. } => scope,
        }
    }
    pub fn revision(&self) -> i64 {
        match self {
            Self::ApproveRuntime { revision, .. }
            | Self::Approve { revision, .. }
            | Self::Revoke { revision, .. } => *revision,
        }
    }
    pub fn profile(&self) -> &Profile {
        match self {
            Self::ApproveRuntime { evidence, .. } | Self::Approve { evidence, .. } => {
                &evidence.collection.profile
            }
            Self::Revoke { profile, .. } => profile,
        }
    }
    pub fn approved(&self) -> bool {
        matches!(self, Self::Approve { .. } | Self::ApproveRuntime { .. })
    }
    pub fn bytes(&self) -> Result<Vec<u8>, Error> {
        self.profile().canonical_bytes().map_err(|_| Error)?;
        let attestation = match self {
            Self::ApproveRuntime { attestation, .. }
            | Self::Approve { attestation, .. }
            | Self::Revoke { attestation, .. } => attestation,
        };
        if !token(self.scope()) || self.revision() <= 0 || !pin(attestation) {
            return Err(Error);
        }
        if let Self::Approve { evidence, .. } | Self::ApproveRuntime { evidence, .. } = self {
            let c = &evidence.collection;
            if [&c.suite, &c.preparation, &c.manifest, &c.helper]
                .into_iter()
                .any(|s| !pin(s))
                || c.cases.len() > 30
                || c.safety.len() > 4
                || evidence
                    .reviews
                    .iter()
                    .any(|r| !token(&r.reviewer) || r.ratings.len() > 30)
            {
                return Err(Error);
            }
            for case in c.cases.iter().chain(&c.safety) {
                if !token(&case.id)
                    || !pin(&case.input)
                    || matches!(&case.candidate,Observation::Generated(p) if p.len()>65536)
                {
                    return Err(Error);
                }
            }
            if evidence
                .reviews
                .iter()
                .flat_map(|r| &r.ratings)
                .any(|r| !token(&r.id) || !pin(&r.presentation))
            {
                return Err(Error);
            }
        }
        let mut bytes = canonical_json::to_vec(&if matches!(self, Self::ApproveRuntime { .. }) {
            Wire::V2(self)
        } else {
            Wire::V1(self)
        })
        .map_err(|_| Error)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_COMMAND_BYTES {
            return Err(Error);
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_COMMAND_BYTES {
            return Err(Error);
        }
        let wire: Wire<Self> = serde_json::from_slice(bytes).map_err(|_| Error)?;
        let value = match wire {
            Wire::V1(value) | Wire::V2(value) => value,
        };
        if value.bytes()? != bytes {
            return Err(Error);
        }
        Ok(value)
    }
}
