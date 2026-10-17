//! Explicit deployment-time planning and create-only JetStream provisioning.
//!
//! Runtime adapters never call this module. Use a single quiesced provisioning
//! writer and separately installed permissions; cancellation can leave a partial
//! plan applied, never permission to delete, roll back, or silently update it.

mod error;
mod permissions;
mod plan;
mod provision;

pub use error::{TopologyError, TopologyErrorKind};
pub use permissions::SubjectPermissions;
pub use plan::{Binding, ConsumerPlan, StreamPlan, TopologyLimits, TopologyPlan};
pub use provision::provision;

/// Broker header reserve above the portable envelope-body ceiling.
///
/// NATS counts protocol headers in stream `max_msg_size`. The pinned publisher
/// adds a bounded source/id deduplication header; boundary proofs exercise its
/// longest supported identity. Raw intake still rejects bodies over 256 KiB.
pub const BROKER_HEADER_RESERVE_BYTES: usize = 1024;

#[cfg(test)]
mod tests;
