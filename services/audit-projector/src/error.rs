//! Payload-safe classification with preserved typed diagnostic causes.
/// Caller-visible failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// Invalid transport/contract.
    Invalid,
    /// Foreign configured caller scope.
    Scope,
    /// Changed immutable request.
    Conflict,
    /// Valid owner input remains unresolved; do not advance successful progress.
    Gap,
    /// Corrupt retained projection.
    Corrupt,
    /// Storage unavailable or transaction uncertain.
    Storage,
}
/// Safe normal diagnostics; source is retained only for explicit trusted diagnosis.
pub struct Error {
    kind: Kind,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}
impl Error {
    /// Classified failure without an invented cause.
    pub const fn new(kind: Kind) -> Self {
        Self { kind, source: None }
    }
    /// Wrap a concrete cause without formatting its private data.
    pub fn caused(kind: Kind, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            kind,
            source: Some(Box::new(source)),
        }
    }
    /// Stable policy category.
    pub const fn kind(&self) -> Kind {
        self.kind
    }
}
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectionError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "projection failed: {:?}", self.kind)
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Self::caused(Kind::Storage, e)
    }
}
impl From<edgeagent_audit_domain::ReplayError> for Error {
    fn from(e: edgeagent_audit_domain::ReplayError) -> Self {
        use edgeagent_audit_domain::ReplayError as R;
        Self::caused(
            match e {
                R::Gap => Kind::Gap,
                R::Conflict => Kind::Conflict,
                R::Scope => Kind::Scope,
                _ => Kind::Corrupt,
            },
            e,
        )
    }
}
