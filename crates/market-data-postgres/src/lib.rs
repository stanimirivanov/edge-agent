//! MarketData-owned immutable admission and serialized account budget transactions.
//! No timers, provider requests, implicit retries or terminal publication live here.
#![deny(missing_docs)]
mod ledger;
pub use ledger::PostgresAcquisitionLedger;

/// Apply in the service-private schema before activating any ledger client.
pub const MIGRATION_SQL: &str = include_str!("../migrations/0001_acquisition_ledger.sql");
