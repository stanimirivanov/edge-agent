//! One-delivery consumer orchestration through persistence-neutral ports.
//!
//! [`handle_once`] owns decoding, route validation, retry, quarantine, and
//! settlement order. [`HandlerPolicy`] validates stable consumer identity and
//! retry bounds. [`HandlingOutcome`] and [`HandlerError`] expose confirmed
//! outcomes and bounded failure categories. The atomic store and broker
//! settlement contracts remain owned by `edgeagent-messaging`.

#![forbid(unsafe_code)]

mod coordinator;
mod error;
mod outcome;
mod policy;
mod resolution;
mod retry;

pub use coordinator::handle_once;
pub use edgeagent_messaging::{HandlerFailure, HandlerFailureKind, QuarantineDisposition};
pub use error::{HandlerError, HandlerErrorKind};
pub use outcome::{HandlingOutcome, MessageFailure};
pub use policy::HandlerPolicy;
