//! Bounded, persistence-neutral outbox relay orchestration.
//!
//! [`relay_once`] coordinates one leased message through portable publisher
//! and store ports. Observation, resolution, policy, retry, outcome, and error modules remain private;
//! the public relay API is re-exported here.

#![forbid(unsafe_code)]

mod coordinator;
mod error;
mod observation;
mod outcome;
mod policy;
mod resolution;
mod retry;

#[cfg(test)]
mod tests;

pub use coordinator::relay_once;
pub use error::{RelayError, RelayErrorKind};
pub use outcome::{QuarantineReason, RelayMessageFailure, RelayOutcome};
pub use policy::RelayPolicy;
