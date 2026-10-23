//! Network-free invented daily evidence and P05 replay. No worker or live activation.
//! The caller supplies invented observations and synthetic authority/reference catalogs.
//! A checksum or this adapter's name cannot establish that third-party data is licensed.
#![deny(missing_docs)]
#![forbid(unsafe_code)]
mod encoding;
use edgeagent_market_data_application::{
    AcquisitionRequest, AuthorizedAcquisition, Cancellation, DailyProvider, KnowledgeMode,
    ProviderBatch, ProviderFailure,
};
use edgeagent_market_data_domain::{CanonicalObservation, PreciseInstant, ReadyEvidence};
use edgeagent_simulation_domain::{ArtifactDigest, DeterministicId, FrozenInput, Run, RunManifest};
pub use encoding::synthetic_daily_bytes;

/// Payload-free fixture/configuration rejection; underlying data is never formatted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OfflineError {
    /// Invalid flag or attempted live activation. No live factory exists here.
    Configuration,
    /// Unsupported, mismatched, empty, oversized or corrupt invented fixture.
    Fixture,
    /// Invalid checkpoint or exhausted deterministic continuation.
    Continuation,
    /// Cancellation requested before advancing the cursor.
    Cancelled,
}
impl std::fmt::Display for OfflineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "offline source rejected: {self:?}")
    }
}
impl std::error::Error for OfflineError {}

/// Explicit offline selection. Absence is safe; a false mock flag is not live authority.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OfflineMode;
impl OfflineMode {
    /// Parse a supplied `USE_MOCK` value without consulting process environment or secrets.
    /// # Errors
    /// Rejects every value except absence, `1` or `true`; no trimming or fallback.
    pub fn from_mock_flag(value: Option<&str>) -> Result<Self, OfflineError> {
        match value {
            None | Some("1" | "true") => Ok(Self),
            _ => Err(OfflineError::Configuration),
        }
    }
}

/// Immutable invented daily fixture, bound to one exact acquisition and a P05 manifest.
/// No URL, credential callback, client, system clock or entropy source is accepted.
pub struct SyntheticDailySource {
    request: AcquisitionRequest,
    manifest: RunManifest,
    observations: Vec<CanonicalObservation>,
    provider: edgeagent_market_data_domain::ProviderCapability,
}
impl std::fmt::Debug for SyntheticDailySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SyntheticDailySource(..)")
    }
}
impl SyntheticDailySource {
    /// Bind invented canonical records to exact retained bytes and checked P04 readiness.
    /// `configuration` must identify synthetic reference/rights/policy/selection revisions;
    /// `implementation` identifies the producer code. Neither digest is authentication.
    /// Only capture-mode provider-day snapshots at the admission clock are supported.
    /// # Errors
    /// Rejects >4096 records, >4 MiB retained bytes, foreign provenance or byte mismatch.
    pub fn new(
        _: OfflineMode,
        request: &AcquisitionRequest,
        ready: &ReadyEvidence<'_>,
        retained: &[Vec<u8>],
        implementation: ArtifactDigest,
        configuration: ArtifactDigest,
        seed: [u8; 32],
    ) -> Result<Self, OfflineError> {
        let records = ready.observations();
        let actual_provider = records
            .first()
            .ok_or(OfflineError::Fixture)?
            .context()
            .origin()
            .1;
        let clock = PreciseInstant::from_millis(request.admitted_at);
        if request.knowledge != KnowledgeMode::Capture
            || ready.assessment().0 != clock
            || records.is_empty()
            || records.len() > 4096
            || retained.len() != records.len()
            || retained
                .iter()
                .try_fold(0_usize, |sum, b| sum.checked_add(b.len()))
                .is_none_or(|bytes| {
                    bytes > 4_194_304 || bytes > request.budget.command_bytes.get() as usize
                })
        {
            return Err(OfflineError::Fixture);
        }
        let mut digests = Vec::with_capacity(records.len());
        for (record, bytes) in records.iter().zip(retained) {
            let (instrument, provider, _) = record.context().origin();
            if instrument != request.scope.instrument
                || provider != actual_provider
                || provider.source() != request.scope.source
                || provider.declaration().0 != request.contract
                || !request
                    .range
                    .contains(record.context().session_conditions().0)
                || synthetic_daily_bytes(record)? != *bytes
            {
                return Err(OfflineError::Fixture);
            }
            digests.push(ArtifactDigest::of(bytes));
        }
        let inputs = FrozenInput::from_ready(ready, &digests).map_err(|_| OfflineError::Fixture)?;
        let binding = encoding::bind_request(request, configuration);
        let manifest = RunManifest::new(clock, implementation, binding, seed, &inputs)
            .map_err(|_| OfflineError::Fixture)?;
        // One daily acquisition cannot use arrival order to resolve conflicting identities.
        let mut observations = Vec::with_capacity(manifest.inputs().len());
        for input in manifest.inputs() {
            let observation = records
                .iter()
                .find(|record| {
                    let (id, revision, _) = record.version().lineage();
                    (id.get(), revision.get()) == input.identity()
                })
                .ok_or(OfflineError::Fixture)?;
            if observations.iter().any(|previous: &CanonicalObservation| {
                previous.version().lineage() == observation.version().lineage()
            }) {
                return Err(OfflineError::Fixture);
            }
            observations.push((*observation).clone());
        }
        Ok(Self {
            request: *request,
            manifest,
            observations,
            provider: actual_provider,
        })
    }
    /// Frozen synthetic provenance, ordering, seed and implementation/configuration binding.
    #[must_use]
    pub const fn manifest(&self) -> &RunManifest {
        &self.manifest
    }
    /// Start a finite single-owner continuation. No durable output commit is implied.
    #[must_use]
    pub fn replay(&self) -> Replay<'_> {
        Replay {
            source: self,
            run: Run::new(&self.manifest),
        }
    }
    /// Restore a caller-committed checkpoint. Persistence must fence stale/concurrent owners.
    /// # Errors
    /// Rejects corrupt, changed-manifest or impossible checkpoints through P05 validation.
    pub fn restore(&self, checkpoint: &[u8]) -> Result<Replay<'_>, OfflineError> {
        let run =
            Run::restore(&self.manifest, checkpoint).map_err(|_| OfflineError::Continuation)?;
        Ok(Replay { source: self, run })
    }
}
impl DailyProvider for SyntheticDailySource {
    async fn fetch(
        &self,
        request: &AuthorizedAcquisition<'_>,
        cancellation: &Cancellation,
    ) -> Result<ProviderBatch, ProviderFailure> {
        if cancellation.is_cancelled() {
            return Err(ProviderFailure::Cancelled);
        }
        if request.request() != &self.request
            || request.authority().1.value().capability != self.provider
        {
            return Err(ProviderFailure::Unsupported);
        }
        Ok(ProviderBatch {
            request: self.request.id,
            provider: self.provider,
            observations: self.observations.clone(),
        })
    }
}

/// P05 single-owner cursor; persist checkpoint and accepted outputs atomically outside here.
pub struct Replay<'a> {
    source: &'a SyntheticDailySource,
    run: Run<'a>,
}
impl Replay<'_> {
    /// Virtual availability clock, never the host wall clock.
    #[must_use]
    pub const fn now(&self) -> PreciseInstant {
        self.run.now()
    }
    /// Versioned P05 continuation bytes, not a distributed ownership token.
    #[must_use]
    pub fn checkpoint(&self) -> Vec<u8> {
        self.run.checkpoint()
    }
    /// Consume the next input with the existing deterministic ID/RNG streams.
    /// # Errors
    /// Cancellation or continuation failure leaves the cursor/checkpoint unchanged.
    pub fn next(
        &mut self,
        cancellation: &Cancellation,
    ) -> Result<Option<ReplayStep<'_>>, OfflineError> {
        if cancellation.is_cancelled() {
            return Err(OfflineError::Cancelled);
        }
        let index = self.run.consumed_inputs().len();
        let Some(observation) = self.source.observations.get(index) else {
            return Ok(None);
        };
        let mut staged = Run::restore(&self.source.manifest, &self.run.checkpoint())
            .map_err(|_| OfflineError::Continuation)?;
        staged.next_input().ok_or(OfflineError::Continuation)?;
        let id = staged.next_id().map_err(|_| OfflineError::Continuation)?;
        let random = staged
            .random_u64()
            .map_err(|_| OfflineError::Continuation)?;
        self.run = staged;
        Ok(Some(ReplayStep {
            observation,
            id,
            random,
        }))
    }
}
/// One invented canonical observation and deterministic continuation values.
pub struct ReplayStep<'a> {
    /// Original canonical observation, including synthetic source/semantic provenance.
    pub observation: &'a CanonicalObservation,
    /// Stable P05 effect identity; not authentication or an observation replacement ID.
    pub id: DeterministicId,
    /// Reproducible public simulation draw; never security entropy.
    pub random: u64,
}
