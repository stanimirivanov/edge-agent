//! MarketData-owned immutable admission and serialized account budget transactions.
//! No timers, provider requests or implicit retries live here.
#![deny(missing_docs)]
mod ledger;
mod qualification;
mod runtime;
mod streaming;
pub use runtime::{QualificationCleanup, QualificationSnapshot, QualificationWorker};
pub use streaming::{
    PostgresStreamingLedger, StreamHistoryEntry, StreamRuntimePhase, StreamingStatus,
};
mod retention;
mod terminal;
pub use ledger::PostgresAcquisitionLedger;

/// Apply in the service-private schema before activating any ledger client.
pub const MIGRATION_SQL: &str = include_str!("../migrations/0001_acquisition_ledger.sql");
/// Additive terminal receipt and ready-publication fences, applied after the ledger.
pub const TERMINAL_MIGRATION_SQL: &str =
    include_str!("../migrations/0002_terminal_publication.sql");
/// Retirement inventory, opaque denial identities and account block generations.
pub const RETENTION_MIGRATION_SQL: &str = include_str!("../migrations/0003_evidence_retention.sql");
/// Ordered private migrations; retention refuses un-inventoried legacy publication metadata.
pub const MIGRATIONS: [&str; 10] = [
    MIGRATION_SQL,
    TERMINAL_MIGRATION_SQL,
    RETENTION_MIGRATION_SQL,
    include_str!("../migrations/0004_native_preparation.sql"),
    include_str!("../migrations/0005_qualification_owner.sql"),
    include_str!("../migrations/0006_qualification_runtime.sql"),
    include_str!("../migrations/0007_streaming_sessions.sql"),
    include_str!("../migrations/0008_stream_publication.sql"),
    include_str!("../migrations/0009_stream_recovery.sql"),
    include_str!("../migrations/0010_stream_owner_inputs.sql"),
];
