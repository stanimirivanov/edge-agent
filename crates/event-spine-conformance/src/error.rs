use std::{error::Error, fmt};

/// Bounded failures at the synthetic acceptance contract boundary.
///
/// No variant retains input, schema URI, parser text, or validator instance data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AcceptanceError {
    /// Structured bytes do not satisfy the portable envelope contract.
    Envelope,
    /// Type, schema, producer, partition, or size is not supported by this registry.
    Routing,
    /// Payload shape or value does not satisfy the selected local schema.
    Payload,
    /// Payload aggregate identity differs from the envelope's partition.
    Partition,
    /// A compiled-in definition or generated schema is invalid.
    Definition,
    /// A candidate changes a retained field/constraint or adds a required field.
    Compatibility,
}

impl fmt::Display for AcceptanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Envelope => "invalid acceptance envelope",
            Self::Routing => "unsupported acceptance route",
            Self::Payload => "invalid acceptance payload",
            Self::Partition => "acceptance aggregate does not match partition",
            Self::Definition => "invalid local acceptance definition",
            Self::Compatibility => "candidate is not an optional-only schema addition",
        })
    }
}

impl Error for AcceptanceError {}
