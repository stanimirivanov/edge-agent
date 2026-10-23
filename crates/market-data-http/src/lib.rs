//! Exact, bounded provider HTTP primitives, not a qualified vendor adapter or budget ledger.
//! Requests require `HttpTransport::start_metered`; no unmetered session is exported.
//! ```compile_fail
//! use edgeagent_market_data_http::HttpSession;
//! ```
#![deny(missing_docs)]
mod endpoint;
mod error;
mod json;
mod metered;
mod transport;
pub use error::{HttpError, HttpErrorKind};
pub use json::{JsonDocument, price, volume};
pub use metered::{DispatchError, DispatchUsage, MeteredSession};
pub use transport::{HttpConfig, HttpTransport, HttpUsage};

#[cfg(test)]
mod tests;
