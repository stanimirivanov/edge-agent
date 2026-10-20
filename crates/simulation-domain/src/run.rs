use edgeagent_market_data_domain::PreciseInstant;
use rand_chacha::{
    ChaCha20Rng,
    rand_core::{Rng, SeedableRng},
};

use crate::{ArtifactDigest, FrozenInput, RunError, RunManifest};

/// Full-width SHA-256 run/counter identity. Not an authorization token or UUID.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DeterministicId([u8; 32]);
impl DeterministicId {
    /// Exact bytes. Namespace is the entire frozen manifest; counter starts at 0.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Single-owner continuation. No background tasks, clocks, I/O or hidden entropy.
/// Persist domain effects, outputs and `checkpoint()` together; discard uncommitted
/// state after cancellation/crash. Forking the same checkpoint repeats identities:
/// this value provides reproducibility, not distributed fencing or exactly once.
///
/// ```
/// use edgeagent_simulation_domain::{Run, RunError, RunManifest};
/// fn resume(manifest: &RunManifest, committed: &[u8]) -> Result<(), RunError> {
///     let mut run = Run::restore(manifest, committed)?;
///     if let Some(input) = run.next_input() {
///         assert_eq!(input.available_at(), run.now());
///         let stable_effect_id = run.next_id()?;
///         // Atomically retain domain state, outputs and run.checkpoint().
///     }
///     Ok(())
/// }
/// ```
pub struct Run<'a> {
    manifest: &'a RunManifest,
    cursor: usize,
    clock: PreciseInstant,
    ids: u64,
    draws: u64,
}
impl<'a> Run<'a> {
    /// Start at the manifest clock with empty ID/random streams.
    #[must_use]
    pub const fn new(manifest: &'a RunManifest) -> Self {
        Self {
            manifest,
            cursor: 0,
            clock: manifest.start,
            ids: 0,
            draws: 0,
        }
    }
    /// Current virtual time. Never reads the operating-system clock.
    #[must_use]
    pub const fn now(&self) -> PreciseInstant {
        self.clock
    }
    /// Frozen run identity for consumers binding virtual time to their own state.
    #[must_use]
    pub const fn manifest_digest(&self) -> ArtifactDigest {
        self.manifest.digest()
    }
    /// Canonical input prefix already consumed by this continuation. Consumers
    /// can bind ready evidence to frozen provenance rather than trusting time alone.
    #[must_use]
    pub fn consumed_inputs(&self) -> &[FrozenInput] {
        // Constructors/restore bound the private cursor; next_input advances only
        // after a successful manifest lookup, so this prefix always exists.
        &self.manifest.inputs[..self.cursor]
    }
    /// Next input without advancing time or exposing its retained payload.
    #[must_use]
    pub fn peek(&self) -> Option<&'a FrozenInput> {
        self.manifest.inputs.get(self.cursor)
    }
    /// Advance virtual time without skipping an unconsumed input. Same-time events
    /// remain ordered by the manifest. Scheduling policies must pin their own order.
    ///
    /// # Errors
    /// Returns `Clock` on regression or an attempt to pass the next availability.
    pub fn advance_to(&mut self, at: PreciseInstant) -> Result<(), RunError> {
        if at < self.clock || self.peek().is_some_and(|next| at > next.available) {
            return Err(RunError::Clock);
        }
        self.clock = at;
        Ok(())
    }
    /// Consume one input at its availability, or return `None` at completion.
    /// Only now may the caller load/verify the retained payload for domain work.
    pub fn next_input(&mut self) -> Option<&'a FrozenInput> {
        let input = self.peek()?;
        self.clock = input.available;
        self.cursor += 1;
        Some(input)
    }
    /// Allocate a stable ID independent of the random stream.
    ///
    /// # Errors
    /// Returns `Exhausted` without mutating state at the counter limit.
    pub fn next_id(&mut self) -> Result<DeterministicId, RunError> {
        let next = self.ids.checked_add(1).ok_or(RunError::Exhausted)?;
        let mut bytes = b"EA-ID1\0".to_vec();
        bytes.extend(self.manifest.digest().bytes());
        bytes.extend(self.ids.to_be_bytes());
        let id = DeterministicId(ArtifactDigest::of(&bytes).bytes());
        self.ids = next;
        Ok(id)
    }
    /// Next portable `ChaCha20` u64: stream 0, word position `2 * draw_count`.
    /// Never use the public simulation seed for secrets or authentication.
    ///
    /// # Errors
    /// Returns `Exhausted` before wrap; no cached generator state needs retention.
    pub fn random_u64(&mut self) -> Result<u64, RunError> {
        let next = self.draws.checked_add(1).ok_or(RunError::Exhausted)?;
        let mut rng = ChaCha20Rng::from_seed(self.manifest.seed);
        rng.set_word_pos(u128::from(self.draws) * 2);
        let value = rng.next_u64();
        self.draws = next;
        Ok(value)
    }
    /// Versioned fixed-width continuation bytes, with SHA-256 corruption check.
    /// The checksum is not authentication; storage/authorization is caller-owned.
    #[must_use]
    pub fn checkpoint(&self) -> Vec<u8> {
        let mut bytes = b"EAC1".to_vec();
        bytes.extend(self.manifest.digest().bytes());
        bytes.extend((self.cursor as u64).to_be_bytes());
        let (seconds, nanos) = self.clock.unix_parts();
        bytes.extend(seconds.to_be_bytes());
        bytes.extend(nanos.to_be_bytes());
        bytes.extend(self.ids.to_be_bytes());
        bytes.extend(self.draws.to_be_bytes());
        bytes.extend(ArtifactDigest::of(&bytes).bytes());
        bytes
    }
    /// Validate fixed length, version, checksum, manifest and legal clock/cursor.
    /// A valid older checkpoint is allowed: durable ownership must prevent rollback
    /// or concurrent writers. Changed manifests cannot resume an old run.
    ///
    /// # Errors
    /// Returns `Checkpoint` for every malformed, mismatched or impossible state.
    pub fn restore(manifest: &'a RunManifest, bytes: &[u8]) -> Result<Self, RunError> {
        if bytes.len() != 108 {
            return Err(RunError::Checkpoint);
        }
        let (body, checksum) = bytes.split_at_checked(76).ok_or(RunError::Checkpoint)?;
        if ArtifactDigest::of(body).bytes().as_slice() != checksum {
            return Err(RunError::Checkpoint);
        }
        let mut body = body;
        if take::<4>(&mut body)? != *b"EAC1" || take::<32>(&mut body)? != manifest.digest().bytes()
        {
            return Err(RunError::Checkpoint);
        }
        let cursor = usize::try_from(u64::from_be_bytes(take(&mut body)?))
            .map_err(|_| RunError::Checkpoint)?;
        let seconds = i64::from_be_bytes(take(&mut body)?);
        let nanos = u32::try_from(u64::from_be_bytes(take(&mut body)?))
            .map_err(|_| RunError::Checkpoint)?;
        let clock =
            PreciseInstant::from_unix_parts(seconds, nanos).map_err(|_| RunError::Checkpoint)?;
        let ids = u64::from_be_bytes(take(&mut body)?);
        let draws = u64::from_be_bytes(take(&mut body)?);
        if cursor > manifest.inputs.len()
            || clock < manifest.start
            || cursor
                .checked_sub(1)
                .and_then(|i| manifest.inputs.get(i))
                .is_some_and(|i| clock < i.available)
            || manifest
                .inputs
                .get(cursor)
                .is_some_and(|i| clock > i.available)
        {
            return Err(RunError::Checkpoint);
        }
        Ok(Self {
            manifest,
            cursor,
            clock,
            ids,
            draws,
        })
    }
}
impl std::fmt::Debug for Run<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Run")
            .field("cursor", &self.cursor)
            .finish_non_exhaustive()
    }
}
fn take<const N: usize>(bytes: &mut &[u8]) -> Result<[u8; N], RunError> {
    let (head, tail) = bytes.split_at_checked(N).ok_or(RunError::Checkpoint)?;
    *bytes = tail;
    head.try_into().map_err(|_| RunError::Checkpoint)
}
