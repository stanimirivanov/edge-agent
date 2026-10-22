//! Synthetic M02 acceptance contracts, never production research or trading APIs.
//!
//! All identifiers and times belong to the caller. Decoding checks the existing
//! envelope/registry boundary and a locally generated payload schema before
//! returning a typed value. Schema URIs are identities, not retrieval locations.

#![forbid(unsafe_code)]

pub mod accounts;
mod contracts;
mod error;
pub mod evidence;
pub mod replay;
pub mod schemas;

pub use contracts::{AcceptanceMessage, ApplyProbe, ProbeApplied, definitions, registry};
pub use error::AcceptanceError;
