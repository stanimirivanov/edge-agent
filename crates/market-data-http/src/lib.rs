//! Exact, bounded provider HTTP and offline-conformant native daily protocol adapters.
//! Installing an adapter grants no live activation, qualification or data rights.
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
mod twelve_data;
pub use error::{HttpError, HttpErrorKind};
pub use json::{JsonDocument, price, volume};
pub use metered::{DispatchError, DispatchUsage, MeteredSession};
pub use transport::{HttpConfig, HttpTransport, HttpUsage};
pub use twelve_data::{TwelveDataConfig, TwelveDataDaily, TwelveDataSession};

#[cfg(test)]
mod tests;
