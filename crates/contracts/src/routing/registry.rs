//! Exact-version message-definition lookup and envelope eligibility.

use super::{MessageDefinition, MessageRoutingError};
use crate::MessageEnvelope;
/// Validated lookup table for the message definitions supported by a process.
/// Static definitions are checked at construction and cannot be mutated while
/// the registry borrows them, so per-envelope checks need only inspect data.
#[derive(Clone, Copy, Debug)]
pub struct MessageRegistry<'definitions> {
    definitions: &'definitions [MessageDefinition],
}

impl<'definitions> MessageRegistry<'definitions> {
    /// Validate definitions and reject duplicate message types or subjects.
    ///
    /// # Examples
    ///
    /// ```
    /// use edgeagent_contracts::{Component, MessageDefinition, MessageRegistry};
    ///
    /// let definitions = [MessageDefinition::command(
    ///     "com.edgeagent.execution.submit-dry-run-order.v1",
    ///     "urn:edgeagent:schema:submit-dry-run-order:v1",
    ///     Component::ExecutionSimulator,
    ///     "order",
    /// )];
    /// let registry = MessageRegistry::new(&definitions)?;
    /// assert_eq!(registry.resolve(definitions[0].message_type()), Some(&definitions[0]));
    /// assert!(registry.resolve("com.edgeagent.execution.unknown.v1").is_none());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error when a definition is invalid or two entries collide.
    pub fn new(
        definitions: &'definitions [MessageDefinition],
    ) -> Result<Self, MessageRoutingError> {
        for (index, definition) in definitions.iter().enumerate() {
            definition.validate()?;
            let subject = definition.subject()?;
            for previous in &definitions[..index] {
                if previous.message_type() == definition.message_type() {
                    return Err(MessageRoutingError::DuplicateMessageType);
                }
                if previous.subject()? == subject {
                    return Err(MessageRoutingError::DuplicateSubject);
                }
            }
        }
        Ok(Self { definitions })
    }

    /// Resolve one exact major-version message type.
    #[must_use]
    pub fn resolve(&self, message_type: &str) -> Option<&'definitions MessageDefinition> {
        self.definitions
            .iter()
            .find(|definition| definition.message_type() == message_type)
    }

    /// Return every validated definition in declaration order.
    #[must_use]
    pub const fn definitions(&self) -> &'definitions [MessageDefinition] {
        self.definitions
    }

    /// Resolve and validate an untrusted envelope.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported message type or contract mismatch.
    pub fn validate(
        &self,
        envelope: &MessageEnvelope,
    ) -> Result<&'definitions MessageDefinition, MessageRoutingError> {
        let definition = self
            .resolve(envelope.message_type())
            .ok_or(MessageRoutingError::UnsupportedMessageType)?;
        definition.validate_envelope_validated(envelope)?;
        Ok(definition)
    }
}
