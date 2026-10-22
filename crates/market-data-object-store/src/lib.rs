//! Create-only, scoped evidence storage. No cloud credentials or public download API.
//! The injected backend must support atomic conditional creation and durable reads.
#![deny(missing_docs)]
mod store;
pub use store::ImmutableEvidenceStore;
