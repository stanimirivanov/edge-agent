//! Stable operational contracts shared by EdgeAgent deployables.

#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt::{Display, Formatter};

mod message_type;
mod messaging;
mod routing;

pub use messaging::{MessageContractError, MessageEnvelope, MessageMetadata};
pub use routing::{
    DeliveryMode, MAX_PORTABLE_MESSAGE_BYTES, MessageDefinition, MessageKind, MessageRegistry,
    MessageRoutingError, RetentionClass, RetentionMode,
};

/// Version of the machine-readable component description contract.
pub const COMPONENT_CONTRACT_VERSION: &str = "edgeagent.component.v1";

/// A stable identifier for an independently deployable component.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Component {
    /// Request ingress and query composition.
    Gateway,
    /// Market-data acquisition, normalization, and evidence snapshots.
    MarketData,
    /// Deterministic analytics, strategy, policy, and artifact publication.
    Research,
    /// Dry-run order lifecycle and simulated portfolio state.
    ExecutionSimulator,
    /// Rebuildable audit and query projections.
    AuditProjector,
}

impl Component {
    /// Return the stable external identifier for this component.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gateway => "edgeagent.gateway",
            Self::MarketData => "edgeagent.market-data",
            Self::Research => "edgeagent.research",
            Self::ExecutionSimulator => "edgeagent.execution-simulator",
            Self::AuditProjector => "edgeagent.audit-projector",
        }
    }

    /// Return the stable CloudEvents source URI for this component.
    #[must_use]
    pub const fn source_uri(self) -> &'static str {
        match self {
            Self::Gateway => "urn:edgeagent:component:gateway",
            Self::MarketData => "urn:edgeagent:component:market-data",
            Self::Research => "urn:edgeagent:component:research",
            Self::ExecutionSimulator => "urn:edgeagent:component:execution-simulator",
            Self::AuditProjector => "urn:edgeagent:component:audit-projector",
        }
    }
}

/// Static metadata exposed by every EdgeAgent service binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceDescriptor {
    /// Stable component identity.
    pub component: Component,
    /// One-line responsibility statement.
    pub summary: &'static str,
    /// Stable capability identifiers implemented by the component.
    pub capabilities: &'static [&'static str],
}

impl ServiceDescriptor {
    /// Construct static service metadata.
    #[must_use]
    pub const fn new(
        component: Component,
        summary: &'static str,
        capabilities: &'static [&'static str],
    ) -> Self {
        Self {
            component,
            summary,
            capabilities,
        }
    }

    /// Validate invariants required by the line-oriented description format.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the summary or capabilities are empty,
    /// duplicated, or unsafe to expose through the stable text contract.
    pub fn validate(self) -> Result<(), DescriptorError> {
        if self.summary.trim().is_empty() {
            return Err(DescriptorError::EmptySummary);
        }
        if self.summary.contains(['\r', '\n', '=']) {
            return Err(DescriptorError::InvalidSummary);
        }
        if self.capabilities.is_empty() {
            return Err(DescriptorError::NoCapabilities);
        }

        for (index, capability) in self.capabilities.iter().enumerate() {
            if !is_token(capability) {
                return Err(DescriptorError::InvalidCapability(capability));
            }
            if self.capabilities[..index].contains(capability) {
                return Err(DescriptorError::DuplicateCapability(capability));
            }
        }
        Ok(())
    }
}

fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Validation failure for [`ServiceDescriptor`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorError {
    /// The responsibility statement is empty.
    EmptySummary,
    /// The responsibility statement cannot be represented safely.
    InvalidSummary,
    /// The service declares no observable capability.
    NoCapabilities,
    /// A capability is not a lowercase ASCII token.
    InvalidCapability(&'static str),
    /// A capability appears more than once.
    DuplicateCapability(&'static str),
}

impl Display for DescriptorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySummary => formatter.write_str("service summary must not be empty"),
            Self::InvalidSummary => {
                formatter.write_str("service summary must be a single line without an equals sign")
            }
            Self::NoCapabilities => formatter.write_str("service must declare a capability"),
            Self::InvalidCapability(value) => {
                write!(formatter, "invalid capability token: {value}")
            }
            Self::DuplicateCapability(value) => {
                write!(formatter, "duplicate capability token: {value}")
            }
        }
    }
}

impl Error for DescriptorError {}

#[cfg(test)]
mod tests {
    use super::{Component, DescriptorError, ServiceDescriptor};

    #[test]
    fn descriptor_rejects_duplicate_capabilities() {
        let descriptor = ServiceDescriptor::new(
            Component::Gateway,
            "Accept requests.",
            &["command-intake", "command-intake"],
        );

        assert_eq!(
            descriptor.validate(),
            Err(DescriptorError::DuplicateCapability("command-intake"))
        );
    }

    #[test]
    fn component_names_are_stable_and_unique() {
        let names = [
            Component::Gateway.as_str(),
            Component::MarketData.as_str(),
            Component::Research.as_str(),
            Component::ExecutionSimulator.as_str(),
            Component::AuditProjector.as_str(),
        ];

        for (index, name) in names.iter().enumerate() {
            assert!(name.starts_with("edgeagent."));
            assert!(!names[..index].contains(name));
        }
    }
}
