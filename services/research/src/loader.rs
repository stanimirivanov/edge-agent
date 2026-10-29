//! Bounded local owner process adapter. Research never opens owner storage or tables.
use edgeagent_contracts::{MessageEnvelope, evidence::*, research_acquisition::ResearchEvidence};
use edgeagent_market_data_domain::{InstrumentId, ListingId, SessionDate, UsdPrice};
use edgeagent_research_application::worker::{EvidenceLoader, LoadError as E, LoadedEvidence};
use edgeagent_research_domain::{
    AdjustmentBasis, EvidencePolicy, FrozenClose, FrozenSeries, PolicyBinding, SelectedInstrument,
};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
/// Immutable operator-selected offline policy and owner endpoint; Debug omits credentials.
pub struct OwnerLoader {
    binary: PathBuf,
    root: PathBuf,
    database: String,
    owner_id: String,
    scope: String,
    implementation: [u8; 32],
    configuration: [u8; 32],
    max_age_ms: i64,
    ttl_ms: i64,
}
fn required(name: &str) -> Result<String, &'static str> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or("missing owner profile setting")
}
/// Decode a nonzero lowercase SHA-256 revision token.
/// # Errors
/// Rejects malformed/revision sentinel values.
pub fn digest(text: &str) -> Result<[u8; 32], E> {
    if text.len() != 64
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(E::Invalid);
    }
    let mut bytes = [0; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| E::Invalid)?;
    }
    if bytes == [0; 32] {
        return Err(E::Invalid);
    }
    Ok(bytes)
}
impl OwnerLoader {
    /// Load an explicit trusted operator profile once, before serving.
    /// # Errors
    /// Returns credential-free configuration failure.
    pub fn from_environment() -> Result<Self, &'static str> {
        let binary = PathBuf::from(required("EDGEAGENT_RESEARCH_OWNER_BINARY")?);
        let root = PathBuf::from(required("EDGEAGENT_RESEARCH_OWNER_ROOT")?);
        if !binary.is_absolute() || !root.is_absolute() {
            return Err("owner requires installed absolute paths");
        }
        let database = required("EDGEAGENT_RESEARCH_OWNER_DATABASE_URL")?;
        crate::runtime::local(&database, &["postgres", "postgresql"])?;
        let owner_id = required("EDGEAGENT_RESEARCH_OWNER_ACQUISITION")?;
        EvidenceId::new(&owner_id).map_err(|_| "invalid owner acquisition")?;
        let scope = required("EDGEAGENT_RESEARCH_SCOPE")?;
        let implementation = digest(&required("EDGEAGENT_RESEARCH_IMPLEMENTATION")?)
            .map_err(|_| "invalid research implementation")?;
        let configuration = digest(&required("EDGEAGENT_RESEARCH_CONFIGURATION")?)
            .map_err(|_| "invalid research configuration")?;
        let max_age_ms = required("EDGEAGENT_RESEARCH_MAX_AGE_MS")?
            .parse()
            .map_err(|_| "invalid max age")?;
        let ttl_ms = required("EDGEAGENT_RESEARCH_TTL_MS")?
            .parse()
            .map_err(|_| "invalid expiry horizon")?;
        if !(1..=2_678_400_000_i64).contains(&max_age_ms) || !(1..=300_000).contains(&ttl_ms) {
            return Err("research policy bounds exceeded");
        }
        Ok(Self {
            binary,
            root,
            database,
            owner_id,
            scope,
            implementation,
            configuration,
            max_age_ms,
            ttl_ms,
        })
    }
    async fn read(&self, delivery: &MessageEnvelope) -> Result<Vec<u8>, E> {
        let mut child = tokio::process::Command::new(&self.binary)
            .args(["offline-research-read", &self.owner_id])
            .env_clear()
            .env("USE_MOCK", "1")
            .env("EDGEAGENT_MARKET_DATA_PROVIDER", "synthetic-daily-v1")
            .env("EDGEAGENT_POSTGRES_URL", &self.database)
            .env("EDGEAGENT_OFFLINE_ROOT", &self.root)
            .env("EDGEAGENT_OFFLINE_RESEARCH_SCOPE", &self.scope)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| E::Unavailable)?;
        let result = tokio::time::timeout(Duration::from_secs(15), async {
            let bytes = delivery.to_json().map_err(|_| E::Invalid)?;
            let mut stdin = child.stdin.take().ok_or(E::Unavailable)?;
            let written = stdin.write_all(&bytes).await;
            drop(stdin);
            let mut bytes = Vec::new();
            child
                .stdout
                .take()
                .ok_or(E::Unavailable)?
                .take((MAX_EVIDENCE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| E::Unavailable)?;
            if bytes.len() > MAX_EVIDENCE_BYTES {
                return Err(E::Invalid);
            }
            let status = child.wait().await.map_err(|_| E::Unavailable)?;
            match status.code() {
                Some(0) if written.is_ok() => Ok(bytes),
                Some(77) => Err(E::Denied),
                Some(65) => Err(E::Invalid),
                _ => Err(E::Unavailable),
            }
        })
        .await
        .unwrap_or(Err(E::Unavailable));
        if result.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        result
    }
}
impl EvidenceLoader for OwnerLoader {
    async fn load(&self, delivery: &MessageEnvelope) -> Result<LoadedEvidence, E> {
        let outcome = ResearchEvidence::from_envelope(delivery).map_err(|_| E::Invalid)?;
        let request = outcome.request();
        if request.scope != self.scope {
            return Err(E::Denied);
        }
        if digest(&request.implementation)? != self.implementation
            || digest(&request.configuration)? != self.configuration
        {
            return Err(E::Unsupported);
        }
        let ResearchEvidence::Ready { decision, .. } = &outcome else {
            return Err(E::Invalid);
        };
        let EvidenceOutcome::Ready { object } = &decision.outcome else {
            return Err(E::Invalid);
        };
        let current = || -> Result<(), E> {
            let now = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| E::Unavailable)?
                    .as_millis(),
            )
            .map_err(|_| E::Unavailable)?;
            if now >= millis(object.delete_by)? {
                return Err(E::Denied);
            }
            Ok(())
        };
        current()?;
        let bytes = self.read(delivery).await?;
        current()?;
        convert(
            delivery,
            &bytes,
            self.implementation,
            self.configuration,
            self.max_age_ms,
            self.ttl_ms,
        )
    }
}
fn millis(t: EvidenceTime) -> Result<i64, E> {
    if !t.nanos.is_multiple_of(1_000_000) {
        return Err(E::Invalid);
    }
    t.seconds
        .checked_mul(1000)
        .and_then(|v| v.checked_add(i64::from(t.nanos / 1_000_000)))
        .ok_or(E::Invalid)
}
/// Convert already owner-authorized exact bytes into pure assertions. This alone grants no read authority.
/// # Errors
/// Rejects digest/descriptor/lineage mismatch, unsupported universe, expiry or malformed facts.
pub fn convert(
    delivery: &MessageEnvelope,
    bytes: &[u8],
    implementation: [u8; 32],
    configuration: [u8; 32],
    max_age_ms: i64,
    ttl_ms: i64,
) -> Result<LoadedEvidence, E> {
    let ResearchEvidence::Ready { request, decision } =
        ResearchEvidence::from_envelope(delivery).map_err(|_| E::Invalid)?
    else {
        return Err(E::Invalid);
    };
    let EvidenceOutcome::Ready { object } = &decision.outcome else {
        return Err(E::Invalid);
    };
    let artifact = DailyEvidenceArtifact::decode(bytes, &object.sha256).map_err(|_| E::Invalid)?;
    if EvidenceDecision::ready(decision.request.clone(), &artifact)
        .map_err(|_| E::Invalid)?
        .data()
        != decision.as_ref()
    {
        return Err(E::Invalid);
    }
    let data = artifact.data();
    if request.instruments != [data.scope.instrument.as_str()]
        || digest(&request.implementation)? != implementation
        || digest(&request.configuration)? != configuration
    {
        return Err(E::Unsupported);
    }
    let n = usize::from(request.lookback_sessions) + 1;
    if data.bars.len() < n {
        return Err(E::Invalid);
    }
    let references = |kind| {
        data.references
            .iter()
            .filter(|r| r.kind == kind)
            .collect::<Vec<_>>()
    };
    let hash = |kind| -> Result<[u8; 32], E> {
        digest(
            DailyEvidenceArtifact::hash(
                &serde_json::to_vec(&references(kind)).map_err(|_| E::Invalid)?,
            )
            .as_str(),
        )
    };
    let selected = SelectedInstrument {
        instrument: InstrumentId::new(
            u128::from_str_radix(data.scope.instrument.as_str(), 16).map_err(|_| E::Invalid)?,
        )
        .map_err(|_| E::Invalid)?,
        listing: ListingId::new(
            u128::from_str_radix(data.scope.listing.as_str(), 16).map_err(|_| E::Invalid)?,
        )
        .map_err(|_| E::Invalid)?,
        mapping: hash(EvidenceReferenceKind::Mapping)?,
    };
    let calendar = hash(EvidenceReferenceKind::Calendar)?;
    let mut known = millis(data.captured_at)?;
    for r in &data.references {
        known = known.max(millis(r.known_at)?);
    }
    let mut closes = Vec::new();
    for bar in &data.bars[data.bars.len() - n..] {
        let parts = bar.date.as_str().split('-').collect::<Vec<_>>();
        let session = SessionDate::new(
            parts[0].parse().map_err(|_| E::Invalid)?,
            parts[1].parse().map_err(|_| E::Invalid)?,
            parts[2].parse().map_err(|_| E::Invalid)?,
        )
        .map_err(|_| E::Invalid)?;
        closes.push(FrozenClose {
            session,
            close: UsdPrice::new(
                bar.ohlc[3].coefficient.parse().map_err(|_| E::Invalid)?,
                u32::from(bar.ohlc[3].scale),
            )
            .map_err(|_| E::Invalid)?,
            event_at_ms: millis(bar.event_at)?,
            observed_at_ms: millis(bar.received_at)?,
            available_at_ms: millis(bar.received_at)?.max(millis(data.captured_at)?),
        });
    }
    let policy = EvidencePolicy::new(
        PolicyBinding {
            decision_at_ms: request.decision_at_ms,
            max_age_ms,
            implementation,
            configuration,
            calendar,
        },
        vec![selected],
        closes.iter().map(|c| c.session).collect(),
        request.lookback_sessions,
    )
    .map_err(|_| E::Invalid)?;
    let rights_until_ms = millis(object.delete_by)?;
    let expires_at_ms = request
        .decision_at_ms
        .checked_add(ttl_ms)
        .ok_or(E::Invalid)?
        .min(rights_until_ms);
    if expires_at_ms <= request.decision_at_ms {
        return Err(E::Denied);
    }
    Ok(LoadedEvidence {
        policy,
        series: vec![FrozenSeries {
            selected,
            evidence: digest(artifact.digest().as_str())?,
            implementation,
            configuration,
            calendar,
            known_at_ms: known,
            rights_until_ms,
            entitled: true,
            quality_accepted: true,
            basis: AdjustmentBasis::Raw,
            closes,
        }],
        expires_at_ms,
    })
}
