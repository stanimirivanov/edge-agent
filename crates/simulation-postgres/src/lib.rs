//! ExecutionSimulator-owned atomic admission adapter. Synthetic quotes only.
//! Bind a verified principal and immutable scenario context at composition, never
//! from payload claims. Use the existing PostgreSQL inbox transaction; ACK only
//! after its commit. This library exposes no fill or reservation-release path.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod context;
mod processing;
mod provision;

pub use context::{AdmissionContext, AdmissionLimits};
pub use provision::{InitialInventory, InitialValuation, provision};

use edgeagent_messaging::{FailureCode, HandlerFailure, HandlerFailureKind};

/// Service-owned migration, applied once in a private schema, not peers' tables.
pub const MIGRATION_SQL: &str = include_str!("../migrations/0001_account_admission.sql");

fn permanent(code: &'static str) -> HandlerFailure {
    HandlerFailure::permanent(FailureCode::from_static(code))
}
fn invariant() -> HandlerFailure {
    permanent("admission_invariant")
}
fn storage(error: sqlx::Error) -> HandlerFailure {
    let transient = match &error {
        sqlx::Error::Database(database) => database.code().is_some_and(|code| {
            code.starts_with("08")
                || code.starts_with("53")
                || matches!(
                    code.as_ref(),
                    "40001" | "40003" | "40P01" | "55P03" | "57014" | "57P01" | "57P02" | "57P03"
                )
        }),
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed
        | sqlx::Error::BeginFailed => true,
        _ => false,
    };
    HandlerFailure::with_source(
        if transient {
            HandlerFailureKind::Transient
        } else {
            HandlerFailureKind::Permanent
        },
        FailureCode::from_static("admission_storage"),
        error,
    )
}
fn signed(value: u64) -> Result<i64, HandlerFailure> {
    i64::try_from(value).map_err(|_| invariant())
}
fn unsigned(value: i64) -> Result<u64, HandlerFailure> {
    u64::try_from(value).map_err(|_| invariant())
}
/// Lowercase fixed-width artifact/identity encoding; never a credential.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    out
}

#[cfg(test)]
mod tests;
