//! Runtime ownership is durable uncertainty, not a renewable lease.
use crate::{PostgresAcquisitionLedger as Ledger, ledger, qualification};
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::UtcInstant;
type Result<T> = std::result::Result<T, BudgetError>;
fn denied() -> BudgetError {
    BudgetError::new(BudgetErrorKind::Denied)
}
fn invalid<T>(_: T) -> BudgetError {
    BudgetError::new(BudgetErrorKind::Invalid)
}

mod assessment;
mod cleanup;
mod input;
mod ownership;
mod preparation;
pub use assessment::QualificationSnapshot;
pub use cleanup::QualificationCleanup;
pub use ownership::QualificationWorker;
pub(crate) use ownership::WorkerBinding;
