use std::{error::Error as StdError, fmt};

/// Payload-free admission/budget rejection classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BudgetErrorKind {
    /// Malformed input or retained state.
    Invalid,
    /// Immutable identity or stale call fence conflicts.
    Conflict,
    /// No matching admitted command.
    NotFound,
    /// An account or acquisition still has an unresolved operation.
    Busy,
    /// Original command/retention deadline elapsed.
    Deadline,
    /// Persisted cancellation prevents new work.
    Cancelled,
    /// An explicit attempt/call/page/byte budget is exhausted.
    Exhausted,
    /// Account start spacing or rolling-minute allowance prevents this start.
    RateLimited,
    /// Database failure; commit may be uncertain. Reload instead of refunding.
    Storage,
    /// An authoritative terminal receipt prevents new acquisition work.
    Terminal,
    /// Current owner-authenticated evidence authority denies publication.
    Denied,
}
/// Safe public diagnostics with deliberately inspectable private storage causes.
pub struct BudgetError {
    pub(super) kind: BudgetErrorKind,
    pub(super) source: Option<Box<dyn StdError + Send + Sync>>,
    pub(super) retry_at_ms: Option<i64>,
}
impl BudgetError {
    /// Construct a source-free policy rejection.
    #[must_use]
    pub fn new(kind: BudgetErrorKind) -> Self {
        Self {
            kind,
            source: None,
            retry_at_ms: None,
        }
    }
    /// Preserve a real adapter cause without displaying it.
    #[must_use]
    pub fn storage(source: impl StdError + Send + Sync + 'static) -> Self {
        Self {
            kind: BudgetErrorKind::Storage,
            source: Some(Box::new(source)),
            retry_at_ms: None,
        }
    }
    /// Preserve a classified concrete cause without exposing its text.
    #[must_use]
    pub fn with_source(
        kind: BudgetErrorKind,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            source: Some(Box::new(source)),
            retry_at_ms: None,
        }
    }
    /// Stable category; never inspect SDK text to select retry behavior.
    #[must_use]
    pub const fn kind(&self) -> BudgetErrorKind {
        self.kind
    }
    /// Earliest database-clock retry instant for rate rejection; never resets a deadline.
    #[must_use]
    pub const fn retry_at_ms(&self) -> Option<i64> {
        self.retry_at_ms
    }
}
impl fmt::Debug for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BudgetError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "acquisition budget: {:?}", self.kind)
    }
}
impl StdError for BudgetError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_deref()
            .map(|s| s as &(dyn StdError + 'static))
    }
}
