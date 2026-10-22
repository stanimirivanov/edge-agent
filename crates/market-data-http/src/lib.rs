//! Exact, bounded provider HTTP primitives, not a qualified vendor adapter or budget ledger.
#![deny(missing_docs)]
mod endpoint;
mod error;
mod json;
mod transport;
pub use error::{HttpError, HttpErrorKind};
pub use json::{JsonDocument, price, volume};
pub use transport::{HttpConfig, HttpSession, HttpTransport, HttpUsage};

#[cfg(test)]
mod tests;
