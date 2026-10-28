//! Immutable daily evidence bytes and identity-only acquisition decisions.
//! Hashes identify bytes, never authenticity, entitlement or a bearer location.
mod artifact;
mod date_only;
mod date_only_daily;
mod messages;
mod model;
mod native_daily;
#[cfg(feature = "schema")]
pub mod schemas;
mod stream;
mod stream_companion;
mod stream_publication;
pub use artifact::DailyEvidenceArtifact;
pub use date_only::*;
pub use date_only_daily::*;
pub use messages::*;
pub use model::*;
pub use native_daily::*;
pub use stream::*;
pub use stream_companion::*;
pub use stream_publication::*;

/// Maximum canonical evidence object bytes; the event envelope stays at 256 KiB.
pub const MAX_EVIDENCE_BYTES: usize = 4 * 1024 * 1024;
/// Payload-free evidence construction, decoding and integrity rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceContractError;
impl std::fmt::Display for EvidenceContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid evidence contract or integrity")
    }
}
impl std::error::Error for EvidenceContractError {}

macro_rules! redacted {
    ($($t:ty),* $(,)?) => {$(impl std::fmt::Debug for $t {
        fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { f.write_str(concat!(stringify!($t),"(..)")) }
    })*};
}
pub(crate) use redacted;
