//! Transactional PostgreSQL outbox storage and relay leasing.
//!
//! This adapter owns exact-byte enqueue, lease-guarded relay transitions,
//! and audited quarantine replay. The crate root retains the stable public API
//! while each storage capability owns its SQL and transaction-scoped methods.

#![forbid(unsafe_code)]

mod enqueue;
mod error;
mod leasing;
mod relay;
mod replay;
mod validation;

#[cfg(test)]
mod tests;

pub use edgeagent_messaging::ClaimedMessage;
pub use enqueue::EnqueueDisposition;
pub use error::{OutboxError, OutboxErrorKind};
pub use relay::PostgresOutboxRelay;
pub use replay::{ReplayDisposition, ReplayRequest};

/// PostgreSQL outbox operations that participate in caller-owned transactions.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostgresOutbox;

impl PostgresOutbox {
    /// Initial SQL migration applied within each service-owned PostgreSQL schema.
    pub const MIGRATION_SQL: &'static str = include_str!("../migrations/0001_message_outbox.sql");

    /// Migration that adds terminal quarantine state to the outbox.
    pub const QUARANTINE_MIGRATION_SQL: &'static str =
        include_str!("../migrations/0002_message_outbox_quarantine.sql");

    /// Migration that adds append-only operator replay audit evidence.
    pub const REPLAY_MIGRATION_SQL: &'static str =
        include_str!("../migrations/0003_message_outbox_replay.sql");

    /// Migration that fences each committed relay claim with a new generation.
    pub const LEASE_GENERATION_MIGRATION_SQL: &'static str =
        include_str!("../migrations/0004_message_outbox_lease_generation.sql");

    /// Ordered migrations required by this adapter.
    pub const MIGRATIONS: [&'static str; 4] = [
        Self::MIGRATION_SQL,
        Self::QUARANTINE_MIGRATION_SQL,
        Self::REPLAY_MIGRATION_SQL,
        Self::LEASE_GENERATION_MIGRATION_SQL,
    ];
}
