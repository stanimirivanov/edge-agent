//! Frozen, finite research/simulation runs without wall clocks or external effects.
//! Manifests bind retained artifacts; they do not qualify evidence or authorize trades.
#![deny(missing_docs)]

mod input;
mod manifest;
mod run;

pub mod admission;
pub mod orders;
pub mod risk;

#[cfg(test)]
mod tests;

pub use input::FrozenInput;
pub use manifest::{ArtifactDigest, RunManifest};
pub use run::{DeterministicId, Run};

/// Payload-free rejection of a deterministic run operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RunError {
    /// Empty, oversized or inconsistent frozen input set.
    Input,
    /// One identity or source sequence names conflicting evidence.
    Conflict,
    /// Time regresses or would skip an unconsumed input.
    Clock,
    /// Checkpoint encoding, manifest binding or continuation is invalid.
    Checkpoint,
    /// A deterministic counter has no remaining values.
    Exhausted,
    /// Supplied retained artifact bytes do not match the pinned digest.
    Artifact,
}
impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "deterministic run rejected: {self:?}")
    }
}
impl std::error::Error for RunError {}
