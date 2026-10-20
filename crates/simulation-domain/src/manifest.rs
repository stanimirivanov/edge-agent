use std::collections::BTreeMap;

use edgeagent_market_data_domain::PreciseInstant;
use sha2::{Digest, Sha256};

use crate::{FrozenInput, RunError};

/// SHA-256 of exact retained artifact bytes, not a qualification or signature.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ArtifactDigest([u8; 32]);
impl ArtifactDigest {
    /// Hash caller-owned canonical bytes. Encoding/version belong to the artifact.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
    /// Preserve an externally retained SHA-256 digest.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    /// Exact SHA-256 bytes for storage and comparison.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
    /// Verify retained content before decoding/using it.
    ///
    /// # Errors
    /// Returns `Artifact` on mismatch. This does not validate content semantics.
    pub fn verify(self, bytes: &[u8]) -> Result<(), RunError> {
        if self == Self::of(bytes) {
            Ok(())
        } else {
            Err(RunError::Artifact)
        }
    }
}
impl std::fmt::Debug for ArtifactDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ArtifactDigest([redacted])")
    }
}

/// Immutable finite run, with v1 canonical big-endian encoding and SHA-256 identity.
/// The configuration artifact must freeze all strategy, policy, calendar,
/// reference, rights, numeric and environment assumptions used by the run.
/// The implementation artifact identifies executable/lockfile revisions.
/// Seeds are public reproducibility inputs, never security entropy.
#[derive(Clone, Eq, PartialEq)]
pub struct RunManifest {
    pub(crate) start: PreciseInstant,
    pub(crate) seed: [u8; 32],
    pub(crate) inputs: Vec<FrozenInput>,
    bytes: Vec<u8>,
    digest: ArtifactDigest,
}
impl RunManifest {
    /// Freeze at most 4096 descriptors (before cloning). Exact duplicates collapse;
    /// conflicting source-scoped identities/sequences reject the entire set.
    /// Availability sorts first, then feed, epoch, optional sequence (`None` first),
    /// observation ID and revision. No event-time sort or arrival-order tie break.
    ///
    /// # Errors
    /// Returns `Input` for empty/oversized sets or pre-start availability, and
    /// `Conflict` for mismatched duplicates or availability-inverted source order.
    pub fn new(
        start: PreciseInstant,
        implementation: ArtifactDigest,
        configuration: ArtifactDigest,
        seed: [u8; 32],
        inputs: &[FrozenInput],
    ) -> Result<Self, RunError> {
        if inputs.is_empty() || inputs.len() > 4096 || inputs.iter().any(|i| i.available < start) {
            return Err(RunError::Input);
        }
        let mut identities = BTreeMap::new();
        let mut sequences = BTreeMap::new();
        let mut scopes = BTreeMap::new();
        let mut ordered = Vec::with_capacity(inputs.len());
        for input in inputs {
            let binding = input.scope_binding();
            if scopes
                .insert((input.feed, input.epoch), binding)
                .is_some_and(|previous| previous != binding)
            {
                return Err(RunError::Conflict);
            }
            let identity = (input.feed, input.epoch, input.identity);
            if let Some(previous) = identities.insert(identity, input) {
                if previous != input {
                    return Err(RunError::Conflict);
                }
                continue;
            }
            if let Some(seq) = input.sequence
                && sequences
                    .insert((input.feed, input.epoch, seq), input)
                    .is_some()
            {
                return Err(RunError::Conflict);
            }
            ordered.push(input.clone());
        }
        ordered.sort_by_key(FrozenInput::key);
        let mut previous_sequences = BTreeMap::new();
        let mut previous_epochs = BTreeMap::new();
        let mut previous_revisions = BTreeMap::new();
        for input in &ordered {
            if previous_epochs
                .insert(input.feed, input.epoch)
                .is_some_and(|previous| previous > input.epoch)
            {
                return Err(RunError::Conflict);
            }
            if previous_revisions
                .insert(
                    (input.feed, input.epoch, input.identity.0),
                    input.identity.1,
                )
                .is_some_and(|previous| previous >= input.identity.1)
            {
                return Err(RunError::Conflict);
            }
            if let Some(seq) = input.sequence
                && previous_sequences
                    .insert((input.feed, input.epoch), seq)
                    .is_some_and(|previous| previous >= seq)
            {
                return Err(RunError::Conflict);
            }
        }
        // Fixed widths, explicit count and magic/version; no platform-native Hash.
        let mut bytes = b"EAR1/SHA256/CHACHA20/IDS-SHA256/ORDER1\0".to_vec();
        let (seconds, nanos) = start.unix_parts();
        bytes.extend(seconds.to_be_bytes());
        bytes.extend(nanos.to_be_bytes());
        bytes.extend(implementation.bytes());
        bytes.extend(configuration.bytes());
        bytes.extend(seed);
        bytes.extend(
            u64::try_from(ordered.len())
                .map_err(|_| RunError::Input)?
                .to_be_bytes(),
        );
        for input in &ordered {
            input.encode(&mut bytes);
        }
        let digest = ArtifactDigest::of(&bytes);
        Ok(Self {
            start,
            seed,
            inputs: ordered,
            bytes,
            digest,
        })
    }
    /// Stable v1 manifest identity, including ordering, evidence and algorithms.
    #[must_use]
    pub const fn digest(&self) -> ArtifactDigest {
        self.digest
    }
    /// Canonical manifest bytes for retention. Reconstruct from retained checked
    /// descriptors and compare these bytes/digest before restoring a checkpoint.
    /// This crate deliberately does not parse untrusted market payloads.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Frozen, deduplicated execution order.
    #[must_use]
    pub fn inputs(&self) -> &[FrozenInput] {
        &self.inputs
    }
}
impl std::fmt::Debug for RunManifest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunManifest")
            .field("inputs", &self.inputs.len())
            .finish_non_exhaustive()
    }
}
