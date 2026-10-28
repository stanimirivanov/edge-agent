//! Complete private owner records; byte consistency is not independent approval.
use super::*;
use edgeagent_contracts::evidence::DailyEvidenceArtifact;
use serde::Deserialize;

/// One retained logical identity scope and its owner-reserved high 64-bit namespace.
#[derive(Clone, Eq, PartialEq)]
pub struct StreamNamespaceClaim {
    namespace: NonZeroU64,
    scope: Vec<u8>,
}
impl StreamNamespaceClaim {
    /// Accept 1..=1024 canonical JSON-array bytes naming the complete logical scope.
    /// Scope interpretation belongs to the provider adapter, not this storage boundary.
    /// # Errors
    /// Rejects malformed/noncanonical/oversized keys before retaining them.
    pub fn new(namespace: NonZeroU64, scope: &[u8]) -> Result<Self> {
        if scope.is_empty() || scope.len() > 1024 {
            return Err(invalid());
        }
        let value: serde_json::Value = serde_json::from_slice(scope).map_err(|_| invalid())?;
        if !value.is_array() || crate::canonical::canonical(value)? != scope {
            return Err(invalid());
        }
        Ok(Self {
            namespace,
            scope: scope.to_vec(),
        })
    }
    /// Exact u64 namespace, including values above PostgreSQL's signed bigint range.
    #[must_use]
    pub const fn namespace(&self) -> u64 {
        self.namespace.get()
    }
    /// Complete private logical-scope bytes. Never log them or treat them as authority.
    #[must_use]
    pub fn scope(&self) -> &[u8] {
        &self.scope
    }
}
/// Bounded complete private record: sealed authority/selection, native profile and claims.
/// This DTO grants no installation, load, capture or readiness capability by itself.
#[derive(Clone, Eq, PartialEq)]
pub struct StreamOwnerInputs {
    id: EvidenceId,
    session: Vec<u8>,
    profile: Vec<u8>,
    claims: Vec<StreamNamespaceClaim>,
    expires: UtcInstant,
    bytes: Vec<u8>,
}
#[derive(Deserialize)]
struct Pin {
    version: String,
    id: EvidenceId,
    configuration: EvidenceDigest,
    window: [i64; 2],
}
type Record = (String, String, String, String, Vec<(String, String)>, i64);
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(value: &str, limit: usize) -> Result<Vec<u8>> {
    if value.len() > limit * 2 || !value.len().is_multiple_of(2) {
        return Err(invalid());
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            if !pair
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
            {
                return Err(invalid());
            }
            u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| invalid())?, 16)
                .map_err(|_| invalid())
        })
        .collect()
}
impl StreamOwnerInputs {
    /// Bind full profile bytes and 1..=128 unique claims to the exact sealed admission.
    /// Expiry is explicit and may only shorten the original capture window.
    /// # Errors
    /// Rejects changed profile pins, duplicate scopes/namespaces, unsupported canonical
    /// session bytes, profiles above 64 KiB or a record above 1 MiB.
    pub fn new(
        session: &StreamingSession,
        profile: &[u8],
        claims: &[StreamNamespaceClaim],
        expires: UtcInstant,
    ) -> Result<Self> {
        if profile.is_empty() || profile.len() > 65536 || claims.is_empty() || claims.len() > 128 {
            return Err(invalid());
        }
        Self::build(
            session.data().id.clone(),
            session.bytes().to_vec(),
            profile.to_vec(),
            claims.to_vec(),
            expires,
        )
    }
    fn build(
        id: EvidenceId,
        session: Vec<u8>,
        profile: Vec<u8>,
        mut claims: Vec<StreamNamespaceClaim>,
        expires: UtcInstant,
    ) -> Result<Self> {
        if session.is_empty()
            || session.len() > 65536
            || profile.is_empty()
            || profile.len() > 65536
            || claims.is_empty()
            || claims.len() > 128
        {
            return Err(invalid());
        }
        let value: serde_json::Value = serde_json::from_slice(&session).map_err(|_| invalid())?;
        if crate::canonical::canonical(value)? != session {
            return Err(invalid());
        }
        let pin: Pin = serde_json::from_slice(&session).map_err(|_| invalid())?;
        if pin.version != "edgeagent.stream-session.v1"
            || pin.id != id
            || pin.configuration != DailyEvidenceArtifact::hash(&profile)
            || expires.unix_millis() <= pin.window[0]
            || expires.unix_millis() > pin.window[1]
        {
            return Err(invalid());
        }
        claims.sort_by_key(StreamNamespaceClaim::namespace);
        if claims.iter().enumerate().any(|(i, c)| {
            claims
                .iter()
                .take(i)
                .any(|old| old.namespace == c.namespace || old.scope == c.scope)
        }) {
            return Err(invalid());
        }
        let fields: Vec<_> = claims
            .iter()
            .map(|c| (format!("{:016x}", c.namespace()), hex(c.scope())))
            .collect();
        let bytes = serde_json::to_vec(&(
            "edgeagent.stream-owner-inputs.v1",
            id.as_str(),
            hex(&session),
            hex(&profile),
            fields,
            expires.unix_millis(),
        ))
        .map_err(|_| invalid())?;
        if bytes.len() > 1_048_576 {
            return Err(invalid());
        }
        Ok(Self {
            id,
            session,
            profile,
            claims,
            expires,
            bytes,
        })
    }
    /// Decode bounded owner-retained bytes and require exact canonical re-encoding.
    /// # Errors
    /// Rejects alternate encodings, unsupported versions or inconsistent admission/profile pins.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 1_048_576 {
            return Err(invalid());
        }
        let (version, id, session, profile, claims, expires): Record =
            serde_json::from_slice(bytes).map_err(|_| invalid())?;
        if version != "edgeagent.stream-owner-inputs.v1" || claims.len() > 128 {
            return Err(invalid());
        }
        let claims = claims
            .into_iter()
            .map(|(namespace, scope)| {
                if namespace.len() != 16
                    || namespace
                        .bytes()
                        .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
                {
                    return Err(invalid());
                }
                let n = u64::from_str_radix(&namespace, 16).map_err(|_| invalid())?;
                StreamNamespaceClaim::new(
                    NonZeroU64::new(n).ok_or_else(invalid)?,
                    &unhex(&scope, 1024)?,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let value = Self::build(
            EvidenceId::new(&id).map_err(|_| invalid())?,
            unhex(&session, 65536)?,
            unhex(&profile, 65536)?,
            claims,
            UtcInstant::from_unix_millis(expires).map_err(|_| invalid())?,
        )?;
        if value.bytes != bytes {
            return Err(invalid());
        }
        Ok(value)
    }
    /// Full canonical private record. Contains reference data; never log it.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Complete native profile bytes, not merely a configuration checksum.
    #[must_use]
    pub fn profile(&self) -> &[u8] {
        &self.profile
    }
    /// Commitment to the complete sealed session, including authority and selection bytes.
    #[must_use]
    pub fn session_digest(&self) -> EvidenceDigest {
        DailyEvidenceArtifact::hash(&self.session)
    }
    /// Exact bounded namespace claims, including complete retained logical scopes.
    #[must_use]
    pub fn claims(&self) -> &[StreamNamespaceClaim] {
        &self.claims
    }
    /// Explicit exclusive expiry; no operation renews it.
    #[must_use]
    pub const fn expires_at(&self) -> UtcInstant {
        self.expires
    }
    /// Recheck exact admission, current capture authority and conservative licensed lifetime.
    /// # Errors
    /// Denies expiry, cancellation, changed authority or retention beyond the current grant.
    pub fn recheck(
        &self,
        session: &StreamingSession,
        catalogs: AuthorityCatalogs<'_>,
        at: UtcInstant,
        cancel: &Cancellation,
    ) -> Result<()> {
        session.recheck(catalogs, at, cancel)?;
        if self.id != session.data().id || self.session != session.bytes() || at >= self.expires {
            return Err(Error::new(K::Denied));
        }
        for scope in &session.data().scopes {
            let remaining = self.expires.unix_millis() - at.unix_millis();
            let mut found = false;
            for record in catalogs.rights.at(at).filter(|r| r.value().scope == *scope) {
                found = true;
                let grant = record.value();
                if self.expires > grant.delete_by
                    || remaining > i64::from(grant.canonical_retention_ms.get().min(86_400_000))
                {
                    return Err(Error::new(K::Denied));
                }
            }
            if !found {
                return Err(Error::new(K::Denied));
            }
        }
        Ok(())
    }
}
macro_rules! redact {($($t:ty),*)=>{$(impl std::fmt::Debug for $t {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(concat!(stringify!($t),"(..)"))}
})*};}
redact!(StreamNamespaceClaim, StreamOwnerInputs);
