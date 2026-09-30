//! Transactional PostgreSQL inbox deduplication for message consumers.
//!
//! This adapter owns delivery recording, exact-byte quarantine, and audited
//! replay authorization. Callers own the surrounding transaction unless they
//! use `PostgresInboundMessageStore`, which commits or rolls back atomically
//! through the persistence-neutral inbound port.

#![forbid(unsafe_code)]

mod delivery;
mod error;
mod handler;
mod quarantine;
mod replay;
mod validation;

#[cfg(test)]
mod tests;

pub use delivery::DeliveryDisposition;
pub use error::{InboxError, InboxErrorKind};
pub use handler::{
    PostgresHandlerFuture, PostgresInboundMessageStore, PostgresTransactionalMessageHandler,
};
pub use quarantine::{QuarantineDisposition, QuarantineEvidence};
pub use replay::{ReplayAuthorization, ReplayDisposition, ReplayRequest};

/// PostgreSQL inbox operations that participate in caller-owned transactions.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostgresInbox;

impl PostgresInbox {
    /// SQL migration applied within each consumer-owned PostgreSQL schema.
    pub const MIGRATION_SQL: &'static str = include_str!("../migrations/0001_message_inbox.sql");

    /// Migration that adds retained inbound poison-message evidence.
    pub const QUARANTINE_MIGRATION_SQL: &'static str =
        include_str!("../migrations/0002_message_quarantine.sql");

    /// Migration that adds append-only inbound replay authorization evidence.
    pub const REPLAY_MIGRATION_SQL: &'static str =
        include_str!("../migrations/0003_message_quarantine_replay.sql");

    /// Ordered migrations required by this adapter.
    pub const MIGRATIONS: [&'static str; 3] = [
        Self::MIGRATION_SQL,
        Self::QUARANTINE_MIGRATION_SQL,
        Self::REPLAY_MIGRATION_SQL,
    ];
}
