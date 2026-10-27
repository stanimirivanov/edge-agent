//! Exact bounded HTTP, frame-charged WSS and offline-conformant native daily adapters.
//! Installing an adapter grants no live activation, qualification or data rights.
//! Requests require `HttpTransport::start_metered`; no unmetered session is exported.
//! ```compile_fail
//! use edgeagent_market_data_http::HttpSession;
//! ```
#![deny(missing_docs)]
mod alpaca;
mod alpaca_iex;
mod iex_acceptance;
pub use alpaca_iex::{
    AlpacaIex, AlpacaIexConfig, AlpacaIexSession, IexError, IexErrorKind, IexObservation,
    IexProgress, IexValue,
};
pub use iex_acceptance::{
    IexAcceptanceError, IexAcceptanceReferences, IexAcceptedObservation, IexCanonicalProfile,
    IexConditionRule, IexIdentityLane, IexIdentityScope,
};
mod endpoint;
pub use alpaca::{AlpacaDaily, AlpacaDailyConfig, AlpacaProgress, AlpacaSession};
mod error;
mod json;
mod metered;
mod transport;
mod twelve_data;
mod wss;
pub use error::{HttpError, HttpErrorKind};
pub use json::{JsonDocument, price, volume};
pub use metered::{DispatchError, DispatchUsage, MeteredSession};
pub use transport::{HttpConfig, HttpTransport, HttpUsage};
pub use twelve_data::{TwelveDataConfig, TwelveDataDaily, TwelveDataSession};
pub use wss::{
    WssConfig, WssError, WssErrorKind, WssFrame, WssFrameKind, WssSession, WssTransport,
};

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../market-data-application/tests/support/streaming.rs"]
mod streaming_fixture;
#[cfg(test)]
mod tests;
