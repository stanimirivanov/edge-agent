//! MarketData-owned immutable admission and serialized account budget transactions.
//! No timers, provider requests, implicit retries or terminal publication live here.
#![deny(missing_docs)]
mod ledger;
mod terminal;
pub use ledger::PostgresAcquisitionLedger;

/// Apply in the service-private schema before activating any ledger client.
pub const MIGRATION_SQL: &str = include_str!("../migrations/0001_acquisition_ledger.sql");
/// Additive terminal receipt and ready-publication fences, applied after the ledger.
pub const TERMINAL_MIGRATION_SQL: &str =
    include_str!("../migrations/0002_terminal_publication.sql");
/// Ordered service-private migrations; compose existing inbox/outbox migrations separately.
pub const MIGRATIONS: [&str; 2] = [MIGRATION_SQL, TERMINAL_MIGRATION_SQL];
