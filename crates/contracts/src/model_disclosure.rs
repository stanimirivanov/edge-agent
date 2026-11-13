//! Non-bearer snapshots for a fresh MarketData model-disclosure owner check.
//! Decoding or binding a snapshot does not authenticate its issuer or authorize use.
use crate::{
    evidence::{EvidenceDigest, EvidenceId},
    explanation::Anchor,
    explanation_profile::Profile,
    messaging::canonical_json,
    research::{ResearchRecord, ResearchRequest, digest, token},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_REQUEST_BYTES: usize = 8192;
pub const MAX_PERMIT_BYTES: usize = 16384;

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    ModelDisclosure,
}

/// Exact disclosure subject and recipient. No endpoint or caller-supplied rights.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Fresh challenge for each authenticated owner check, never a reusable lease.
    pub operation: EvidenceId,
    pub research: ResearchRequest,
    pub anchor: Anchor,
    pub acquisition: EvidenceId,
    /// SHA-256 of the exact retained owner publication bytes.
    pub publication: String,
    pub evidence: EvidenceDigest,
    /// Reviewed logical model audience, independently checked against deployment.
    pub audience: String,
    pub purpose: Purpose,
    pub profile: Profile,
    /// SHA-256 of the compiled preparation/projection contract.
    pub input_contract: String,
    /// SHA-256 of the actual prepared input bytes.
    pub input: String,
}

/// Owner response metadata, only meaningful on an authenticated fresh check.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permit {
    pub request: Request,
    /// Owner-derived digest of the sealed rights/feed scope (ADR 0117).
    pub rights_scope: String,
    pub review_key: String,
    pub review_revision: i64,
    /// Opaque reference to the reviewed agreement; no legal document content.
    pub agreement: EvidenceId,
    pub checked_at_ms: i64,
    pub expires_at_ms: i64,
    pub review_until_ms: i64,
    pub retention_until_ms: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
enum RequestWire<T> {
    #[serde(rename = "market-data.model-disclosure.request.v1")]
    V1(T),
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
enum PermitWire<T> {
    #[serde(rename = "market-data.model-disclosure.permit.v1")]
    V1(T),
}
#[derive(Serialize)]
struct ReviewKey<'a> {
    kind: &'static str,
    rights_scope: &'a str,
    audience: &'a str,
    profile: String,
    deployment: &'a crate::explanation_profile::Deployment,
    input_contract: &'a str,
    purpose: Purpose,
}

#[derive(Debug)]
pub struct Error;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid model-disclosure contract")
    }
}
impl std::error::Error for Error {}

fn pin(value: &str) -> bool {
    digest(value) && value.bytes().any(|byte| byte != b'0')
}
fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = canonical_json::to_vec(value).map_err(|_| Error)?;
    bytes.push(b'\n');
    if bytes.len() > limit {
        return Err(Error);
    }
    Ok(bytes)
}
impl Request {
    fn validate(&self) -> Result<(), Error> {
        ResearchRecord::Request(self.research.clone())
            .canonical_bytes()
            .map_err(|_| Error)?;
        self.profile.canonical_bytes().map_err(|_| Error)?;
        if !self.anchor.is_valid()
            || self.anchor.scope != self.research.scope
            || !token(&self.audience)
            || [&self.publication, &self.input_contract, &self.input]
                .into_iter()
                .any(|v| !pin(v))
        {
            return Err(Error);
        }
        Ok(())
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, Error> {
        self.validate()?;
        encode(&RequestWire::V1(self), MAX_REQUEST_BYTES)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(Error);
        }
        let RequestWire::V1(value): RequestWire<Self> =
            serde_json::from_slice(bytes).map_err(|_| Error)?;
        if value.canonical_bytes()? != bytes {
            return Err(Error);
        }
        Ok(value)
    }
    /// Review identity excludes per-operation payload instances, but pins the full
    /// profile, recipient, rights scope and projection contract. Never a grant.
    pub fn review_key(&self, rights_scope: &str) -> Result<String, Error> {
        self.canonical_bytes()?;
        if !pin(rights_scope) {
            return Err(Error);
        }
        let bytes = encode(
            &ReviewKey {
                kind: "market-data.model-disclosure-review.v1",
                rights_scope,
                audience: &self.audience,
                profile: self.profile.id().map_err(|_| Error)?,
                deployment: &self.profile.deployment,
                input_contract: &self.input_contract,
                purpose: self.purpose,
            },
            MAX_REQUEST_BYTES,
        )?;
        Ok(Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}
impl Permit {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, Error> {
        if !pin(&self.review_key)
            || self.review_key != self.request.review_key(&self.rights_scope)?
            || self.review_revision <= 0
            || self.checked_at_ms < self.request.research.decision_at_ms
            || self.checked_at_ms >= self.expires_at_ms
            || self.expires_at_ms > self.review_until_ms.min(self.retention_until_ms)
        {
            return Err(Error);
        }
        encode(&PermitWire::V1(self), MAX_PERMIT_BYTES)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_PERMIT_BYTES {
            return Err(Error);
        }
        let PermitWire::V1(value): PermitWire<Self> =
            serde_json::from_slice(bytes).map_err(|_| Error)?;
        if value.canonical_bytes()? != bytes {
            return Err(Error);
        }
        Ok(value)
    }
    /// Check subject equality and the exclusive expiry boundary only. The caller
    /// must authenticate a fresh owner response and check current runtime identity,
    /// grants, actual prepared bytes and durable dispatch authority separately.
    pub fn bind(&self, expected: &Request, now_ms: i64) -> Result<(), Error> {
        self.canonical_bytes()?;
        expected.canonical_bytes()?;
        if self.request != *expected || now_ms < self.checked_at_ms || now_ms >= self.expires_at_ms
        {
            return Err(Error);
        }
        Ok(())
    }
}
