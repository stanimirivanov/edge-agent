use std::{error::Error, fmt};

/// Bounded provisioning failures; none contains a route or configuration value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TopologyErrorKind {
    /// Reject invalid limits, names, definitions, or explicit role bindings.
    InvalidInput,
    /// Existing server configuration differs; stop and request operator review.
    Conflict,
    /// I/O failed or its outcome is unknown; inspect and retry the same plan.
    Unavailable,
}

/// Payload-safe failure retaining actual SDK causes for explicit diagnosis only.
pub struct TopologyError {
    kind: TopologyErrorKind,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl TopologyError {
    pub(super) const fn new(kind: TopologyErrorKind) -> Self {
        Self { kind, source: None }
    }

    pub(super) fn io(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            kind: TopologyErrorKind::Unavailable,
            source: Some(Box::new(error)),
        }
    }

    /// Return the category without rendering configuration or SDK text.
    #[must_use]
    pub const fn kind(&self) -> TopologyErrorKind {
        self.kind
    }
}

impl fmt::Display for TopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.kind {
            TopologyErrorKind::InvalidInput => "invalid topology plan",
            TopologyErrorKind::Conflict => "existing topology conflicts with plan",
            TopologyErrorKind::Unavailable => "topology operation unavailable or outcome unknown",
        })
    }
}

impl fmt::Debug for TopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TopologyError")
            .field("kind", &self.kind)
            .field("has_source", &self.source.is_some())
            .finish()
    }
}

impl Error for TopologyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}
