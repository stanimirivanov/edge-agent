//! Conformance-only replay input and broker scope, not a product operator API.

use crate::{AcceptanceError, AcceptanceMessage};
use edgeagent_contracts::{Component, MessageEnvelope};
use edgeagent_messaging_nats::topology::TopologyPlan;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt};

/// Bounded by the finite worker before decoding. Actor identity is never input.
#[derive(Deserialize, Serialize)]
#[serde(tag = "direction", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReplayCommand {
    /// Authorize an exact retained delivery on its owning consumer route.
    Inbound {
        /// Idempotent replay-request identity, not domain message identity.
        request_id: String,
        /// Stable logical subscriber recorded in quarantine.
        consumer_name: String,
        /// Opaque retained broker delivery identity.
        delivery_key: String,
        /// Bounded operator reason code.
        reason: String,
    },
    /// Release an existing quarantined outbox identity through its owning API.
    Outbound {
        /// Idempotent operator request identity.
        request_id: String,
        /// Original CloudEvents source.
        message_source: String,
        /// Original CloudEvents ID.
        message_id: String,
        /// Bounded operator reason code.
        reason: String,
    },
}

impl fmt::Debug for ReplayCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReplayCommand").finish_non_exhaustive()
    }
}

/// Exact-subject replay publisher with no broker administration or intake grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayPublisherScope {
    user: String,
    inbox: String,
    subjects: BTreeSet<String>,
}

impl ReplayPublisherScope {
    /// Derive only this component's incoming routes from the validated plan.
    ///
    /// # Errors
    /// Rejects an absent component or a component with no declared incoming route.
    pub fn new(plan: &TopologyPlan, component: Component) -> Result<Self, AcceptanceError> {
        let runtime = plan
            .runtime_permissions()
            .iter()
            .find(|p| p.component() == Some(component))
            .ok_or(AcceptanceError::Routing)?;
        let subjects = plan
            .streams()
            .iter()
            .filter(|s| s.consumers().iter().any(|c| c.component() == component))
            .flat_map(|s| s.config().subjects.iter().cloned())
            .collect::<BTreeSet<_>>();
        if subjects.is_empty() {
            return Err(AcceptanceError::Routing);
        }
        Ok(Self {
            user: format!("replay_{}", component.as_str().replace('.', "_")),
            // A sibling, not a child of the ordinary runtime inbox: its wildcard
            // subscription must not grant access to replay publication receipts.
            inbox: format!("{}_replay", runtime.inbox_prefix()),
            subjects,
        })
    }

    /// Fixture login to which these exact permissions are attached.
    pub fn user(&self) -> &str {
        &self.user
    }
    /// Private reply inbox prefix, isolated from ordinary runtime subscriptions.
    pub fn inbox_prefix(&self) -> &str {
        &self.inbox
    }
    /// Original subjects this principal may publish to, never a wildcard.
    pub fn subjects(&self) -> &BTreeSet<String> {
        &self.subjects
    }
    /// Credential-free permissions for the exclusively owned proof broker.
    pub fn nats_config(&self) -> String {
        let subjects = self
            .subjects
            .iter()
            .map(|s| format!("{s:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{{ publish: {{ allow: [{subjects}] }}, subscribe: {{ allow: [{:?}] }} }}",
            format!("{}.>", self.inbox)
        )
    }
}

/// Validate exact bytes and original subject against the configured owning route.
///
/// This deliberately performs no repair or envelope re-encoding.
/// # Errors
/// Rejects malformed/unsupported payloads, wrong subjects, or another subscriber.
pub fn validate_inbound(
    plan: &TopologyPlan,
    component: Component,
    consumer: &str,
    subject: &str,
    bytes: &[u8],
) -> Result<(), AcceptanceError> {
    let envelope = MessageEnvelope::from_json(bytes).map_err(|_| AcceptanceError::Envelope)?;
    AcceptanceMessage::from_envelope(&envelope)?;
    let allowed = plan.streams().iter().any(|stream| {
        stream.definition().message_type() == envelope.message_type()
            && stream.config().subjects.iter().any(|s| s == subject)
            && stream.consumers().iter().any(|c| {
                c.component() == component && c.config().durable_name.as_deref() == Some(consumer)
            })
    });
    if allowed {
        Ok(())
    } else {
        Err(AcceptanceError::Routing)
    }
}

#[cfg(test)]
mod tests;
