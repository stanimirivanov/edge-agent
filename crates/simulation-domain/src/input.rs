use edgeagent_market_data_domain::{PreciseInstant, ReadyEvidence};

use crate::{ArtifactDigest, RunError};

/// Immutable input descriptor captured only from a successful P04 readiness check.
/// Retain the corresponding artifact separately; its digest must cover the entire
/// canonical observation and its semantic/reference provenance, not Rust `Debug`.
///
/// ```compile_fail,E0451
/// use edgeagent_simulation_domain::FrozenInput;
/// fn forge(existing: FrozenInput) -> FrozenInput {
///     FrozenInput { feed: 99, ..existing }
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenInput {
    pub(crate) available: PreciseInstant,
    pub(crate) feed: u128,
    pub(crate) epoch: u64,
    pub(crate) sequence: Option<u64>,
    pub(crate) identity: (u128, u64),
    pub(crate) provenance: [u128; 9],
    pub(crate) artifact: ArtifactDigest,
}
impl FrozenInput {
    /// Freeze a complete ready plan in plan order, binding one retained artifact
    /// digest per observation. Availability is the assessment instant, never an
    /// earlier event/provider time. This conservatively preserves recovery delays.
    ///
    /// # Errors
    /// Returns `Input` unless the digest count exactly matches the ready plan.
    pub fn from_ready(
        ready: &ReadyEvidence<'_>,
        artifacts: &[ArtifactDigest],
    ) -> Result<Vec<Self>, RunError> {
        if artifacts.len() != ready.observations().len() {
            return Err(RunError::Input);
        }
        let (available, readiness) = ready.assessment();
        Ok(ready
            .observations()
            .iter()
            .zip(ready.feeds())
            .zip(artifacts)
            .map(|((observation, (feed, epoch, continuity)), artifact)| {
                let (id, revision, _) = observation.version().lineage();
                let (instrument, provider, transformation) = observation.context().origin();
                let (contract, _, _) = provider.declaration();
                let (semantics, _) = contract.rules();
                let (semantic_id, semantic_rev) = semantics.version();
                let (continuity_id, continuity_rev) = continuity.version();
                let (readiness_id, readiness_rev) = readiness.version();
                Self {
                    available,
                    feed: feed.get(),
                    epoch: epoch.get(),
                    sequence: observation.sequence(),
                    identity: (id.get(), revision.get()),
                    provenance: [
                        instrument.get(),
                        provider.source().get(),
                        u128::from(transformation.get()),
                        semantic_id.get(),
                        u128::from(semantic_rev.get()),
                        continuity_id.get(),
                        u128::from(continuity_rev.get()),
                        readiness_id.get(),
                        u128::from(readiness_rev.get()),
                    ],
                    artifact: *artifact,
                }
            })
            .collect())
    }

    /// Conservative point-in-time availability, including readiness barriers.
    #[must_use]
    pub const fn available_at(&self) -> PreciseInstant {
        self.available
    }

    /// Source-scoped logical observation identity and revision.
    #[must_use]
    pub const fn identity(&self) -> (u128, u64) {
        self.identity
    }

    /// Digest of retained observation bytes. Readiness is pinned separately.
    #[must_use]
    pub const fn artifact(&self) -> ArtifactDigest {
        self.artifact
    }
    pub(crate) fn supports_reassessment(&self, assessed: &Self) -> bool {
        // Preserve first availability while allowing a later readiness check of
        // the SAME consumed evidence, e.g. for delayed simulation. No identity,
        // provenance, policy or retained bytes may change during reassessment.
        self.available <= assessed.available
            && self.feed == assessed.feed
            && self.epoch == assessed.epoch
            && self.sequence == assessed.sequence
            && self.identity == assessed.identity
            && self.provenance == assessed.provenance
            && self.artifact == assessed.artifact
    }

    pub(crate) fn key(&self) -> (PreciseInstant, u128, u64, Option<u64>, (u128, u64)) {
        (
            self.available,
            self.feed,
            self.epoch,
            self.sequence,
            self.identity,
        )
    }

    pub(crate) const fn scope_binding(&self) -> [u128; 6] {
        let [
            _,
            source,
            transformation,
            semantics,
            semantic_revision,
            continuity,
            continuity_revision,
            _,
            _,
        ] = self.provenance;
        [
            source,
            transformation,
            semantics,
            semantic_revision,
            continuity,
            continuity_revision,
        ]
    }

    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        let (seconds, nanos) = self.available.unix_parts();
        out.extend(seconds.to_be_bytes());
        out.extend(nanos.to_be_bytes());
        out.extend(self.feed.to_be_bytes());
        out.extend(self.epoch.to_be_bytes());
        out.push(u8::from(self.sequence.is_some()));
        out.extend(self.sequence.unwrap_or(0).to_be_bytes());
        out.extend(self.identity.0.to_be_bytes());
        out.extend(self.identity.1.to_be_bytes());
        for field in self.provenance {
            out.extend(field.to_be_bytes());
        }
        out.extend(self.artifact.bytes());
    }
}
