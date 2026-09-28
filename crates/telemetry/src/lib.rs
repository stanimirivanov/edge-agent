//! Bounded, exporter-neutral telemetry for the EdgeAgent event spine.

#![forbid(unsafe_code)]

use edgeagent_contracts::MessageEnvelope;
use edgeagent_messaging::DeliveryMetadata;
use std::time::Duration;

/// Counter for completed event-spine stages.
pub const EVENT_SPINE_OPERATIONS_TOTAL: &str = "edgeagent.event_spine.operations";
/// Histogram for completed event-spine stage duration in seconds.
pub const EVENT_SPINE_OPERATION_DURATION_SECONDS: &str = "edgeagent.event_spine.operation.duration";

/// Bounded event-spine stage used as a metric dimension and trace field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventSpineStage {
    /// A broker publication attempt reached a classified result.
    Publication,
    /// A durable outbox or inbox state transition committed.
    Persistence,
    /// Transactional message handling reached a classified result.
    Handling,
    /// A broker delivery settlement reached a confirmed result.
    Acknowledgement,
}

impl EventSpineStage {
    /// Return the stable bounded telemetry token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Publication => "publication",
            Self::Persistence => "persistence",
            Self::Handling => "handling",
            Self::Acknowledgement => "acknowledgement",
        }
    }
}

/// Bounded outcome used as a metric dimension and trace field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventSpineOutcome {
    /// The stage completed its requested transition.
    Succeeded,
    /// Stable identity suppressed duplicate work.
    Duplicate,
    /// Durable policy scheduled another attempt.
    RetryScheduled,
    /// Durable evidence retained a terminal failure.
    Quarantined,
    /// The stage returned without a confirmed durable outcome.
    Failed,
}

impl EventSpineOutcome {
    /// Return the stable bounded telemetry token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Duplicate => "duplicate",
            Self::RetryScheduled => "retry_scheduled",
            Self::Quarantined => "quarantined",
            Self::Failed => "failed",
        }
    }
}

/// Correlation fields allowed on event-spine diagnostic events.
///
/// Values are never metric dimensions. Payloads, subjects, idempotency keys,
/// failure text, user identifiers, and market identifiers are intentionally
/// absent so an exporter cannot accidentally create sensitive or unbounded
/// metric series through this API.
pub struct EventSpineContext {
    message_id: Option<String>,
    message_source: Option<String>,
    message_type: Option<String>,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    trace_parent: Option<String>,
    transport_message_key: Option<String>,
}

impl EventSpineContext {
    /// Capture approved correlation fields from a validated envelope.
    #[must_use]
    pub fn from_envelope(envelope: &MessageEnvelope) -> Self {
        Self {
            message_id: Some(envelope.id().to_owned()),
            message_source: Some(envelope.source().to_owned()),
            message_type: Some(envelope.message_type().to_owned()),
            correlation_id: envelope.extension("correlationid").map(str::to_owned),
            causation_id: envelope.extension("causationid").map(str::to_owned),
            trace_parent: envelope.extension("traceparent").map(str::to_owned),
            transport_message_key: None,
        }
    }

    /// Capture bounded transport identity before an envelope is trusted.
    #[must_use]
    pub fn from_delivery(metadata: &DeliveryMetadata) -> Self {
        Self {
            message_id: None,
            message_source: None,
            message_type: None,
            correlation_id: None,
            causation_id: None,
            trace_parent: None,
            transport_message_key: Some(metadata.message_key().to_owned()),
        }
    }

    /// Capture denormalized identity for an outbox record whose bytes may be invalid.
    #[must_use]
    pub fn from_stored_identity(source: &str, id: &str, message_type: &str) -> Self {
        Self {
            message_id: Some(id.to_owned()),
            message_source: Some(source.to_owned()),
            message_type: Some(message_type.to_owned()),
            correlation_id: None,
            causation_id: None,
            trace_parent: None,
            transport_message_key: None,
        }
    }

    /// Return the message ID when a validated or stored identity is available.
    #[must_use]
    pub fn message_id(&self) -> Option<&str> {
        self.message_id.as_deref()
    }

    /// Return the message source when a validated or stored identity is available.
    #[must_use]
    pub fn message_source(&self) -> Option<&str> {
        self.message_source.as_deref()
    }

    /// Return the registered message type when available.
    #[must_use]
    pub fn message_type(&self) -> Option<&str> {
        self.message_type.as_deref()
    }

    /// Return the workflow correlation ID from a validated envelope.
    #[must_use]
    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// Return the direct-cause ID from a validated envelope.
    #[must_use]
    pub fn causation_id(&self) -> Option<&str> {
        self.causation_id.as_deref()
    }

    /// Return the validated W3C parent context when available.
    #[must_use]
    pub fn trace_parent(&self) -> Option<&str> {
        self.trace_parent.as_deref()
    }

    /// Return the opaque transport identity for an untrusted delivery.
    #[must_use]
    pub fn transport_message_key(&self) -> Option<&str> {
        self.transport_message_key.as_deref()
    }
}

/// Emit one completed event-spine stage without affecting domain behavior.
///
/// Metrics contain only the bounded `stage` and `outcome` dimensions. Message
/// and trace identifiers are emitted only as structured diagnostic fields.
pub fn record_event_spine_operation(
    context: &EventSpineContext,
    stage: EventSpineStage,
    outcome: EventSpineOutcome,
    attempt: u32,
    duration: Duration,
) {
    metrics::counter!(
        EVENT_SPINE_OPERATIONS_TOTAL,
        "stage" => stage.as_str(),
        "outcome" => outcome.as_str()
    )
    .increment(1);
    metrics::histogram!(
        EVENT_SPINE_OPERATION_DURATION_SECONDS,
        "stage" => stage.as_str(),
        "outcome" => outcome.as_str()
    )
    .record(duration.as_secs_f64());

    tracing::event!(
        target: "edgeagent.event_spine",
        tracing::Level::INFO,
        event_name = "edgeagent.event_spine.operation",
        event_stage = stage.as_str(),
        event_outcome = outcome.as_str(),
        delivery_attempt = attempt,
        duration_seconds = duration.as_secs_f64(),
        message_id = context.message_id().unwrap_or(""),
        message_source = context.message_source().unwrap_or(""),
        message_type = context.message_type().unwrap_or(""),
        correlation_id = context.correlation_id().unwrap_or(""),
        causation_id = context.causation_id().unwrap_or(""),
        trace_parent = context.trace_parent().unwrap_or(""),
        transport_message_key = context.transport_message_key().unwrap_or(""),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        EVENT_SPINE_OPERATION_DURATION_SECONDS, EVENT_SPINE_OPERATIONS_TOTAL, EventSpineContext,
        EventSpineOutcome, EventSpineStage, record_event_spine_operation,
    };
    use edgeagent_contracts::{MessageEnvelope, MessageMetadata};
    use metrics::{
        Counter, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit,
    };
    use serde_json::json;
    use std::error::Error;
    use std::io;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Default)]
    struct CapturingRecorder {
        keys: Mutex<Vec<Key>>,
    }

    impl CapturingRecorder {
        fn capture(&self, key: &Key) {
            if let Ok(mut keys) = self.keys.lock() {
                keys.push(key.clone());
            }
        }
    }

    impl Recorder for CapturingRecorder {
        fn describe_counter(
            &self,
            _key_name: KeyName,
            _unit: Option<Unit>,
            _description: SharedString,
        ) {
        }

        fn describe_gauge(
            &self,
            _key_name: KeyName,
            _unit: Option<Unit>,
            _description: SharedString,
        ) {
        }

        fn describe_histogram(
            &self,
            _key_name: KeyName,
            _unit: Option<Unit>,
            _description: SharedString,
        ) {
        }

        fn register_counter(&self, key: &Key, _metadata: &Metadata<'_>) -> Counter {
            self.capture(key);
            Counter::noop()
        }

        fn register_gauge(&self, key: &Key, _metadata: &Metadata<'_>) -> Gauge {
            self.capture(key);
            Gauge::noop()
        }

        fn register_histogram(&self, key: &Key, _metadata: &Metadata<'_>) -> Histogram {
            self.capture(key);
            Histogram::noop()
        }
    }

    fn envelope() -> Result<MessageEnvelope, Box<dyn Error>> {
        Ok(MessageEnvelope::from_payload(
            MessageMetadata {
                id: "message-01".to_owned(),
                source: "urn:edgeagent:component:gateway".to_owned(),
                message_type: "com.edgeagent.research.request-received.v1".to_owned(),
                subject: "research-request/request-01".to_owned(),
                time: "2026-09-28T00:00:00Z".to_owned(),
                data_schema: "urn:edgeagent:schema:research-request-received:v1".to_owned(),
                correlation_id: "correlation-01".to_owned(),
                causation_id: "request-01".to_owned(),
                idempotency_key: "private-idempotency-key".to_owned(),
                partition_key: "research-request/request-01".to_owned(),
                trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
                trace_state: None,
            },
            &json!({"private_payload": "not telemetry"}),
        )?)
    }

    #[test]
    fn context_exposes_only_approved_correlation_fields() -> Result<(), Box<dyn Error>> {
        let context = EventSpineContext::from_envelope(&envelope()?);

        assert_eq!(context.message_id(), Some("message-01"));
        assert_eq!(
            context.message_source(),
            Some("urn:edgeagent:component:gateway")
        );
        assert_eq!(
            context.message_type(),
            Some("com.edgeagent.research.request-received.v1")
        );
        assert_eq!(context.correlation_id(), Some("correlation-01"));
        assert_eq!(context.causation_id(), Some("request-01"));
        assert_eq!(
            context.trace_parent(),
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
        );
        assert_eq!(context.transport_message_key(), None);
        Ok(())
    }

    #[test]
    fn metric_names_and_dimensions_are_stable_and_bounded() {
        assert_eq!(
            EVENT_SPINE_OPERATIONS_TOTAL,
            "edgeagent.event_spine.operations"
        );
        assert_eq!(
            EVENT_SPINE_OPERATION_DURATION_SECONDS,
            "edgeagent.event_spine.operation.duration"
        );
        assert_eq!(EventSpineStage::Publication.as_str(), "publication");
        assert_eq!(EventSpineStage::Persistence.as_str(), "persistence");
        assert_eq!(EventSpineStage::Handling.as_str(), "handling");
        assert_eq!(EventSpineStage::Acknowledgement.as_str(), "acknowledgement");
        assert_eq!(EventSpineOutcome::Succeeded.as_str(), "succeeded");
        assert_eq!(EventSpineOutcome::Duplicate.as_str(), "duplicate");
        assert_eq!(
            EventSpineOutcome::RetryScheduled.as_str(),
            "retry_scheduled"
        );
        assert_eq!(EventSpineOutcome::Quarantined.as_str(), "quarantined");
        assert_eq!(EventSpineOutcome::Failed.as_str(), "failed");
    }

    #[test]
    fn emitted_metrics_exclude_message_identity() -> Result<(), Box<dyn Error>> {
        let recorder = CapturingRecorder::default();
        let context = EventSpineContext::from_envelope(&envelope()?);

        metrics::with_local_recorder(&recorder, || {
            record_event_spine_operation(
                &context,
                EventSpineStage::Handling,
                EventSpineOutcome::Succeeded,
                2,
                Duration::from_millis(25),
            );
        });

        let keys = recorder
            .keys
            .lock()
            .map_err(|_| io::Error::other("capturing recorder lock was poisoned"))?;
        assert_eq!(keys.len(), 2);
        for key in keys.iter() {
            let labels = key
                .labels()
                .map(|label| (label.key(), label.value()))
                .collect::<Vec<_>>();
            assert_eq!(
                labels,
                vec![("stage", "handling"), ("outcome", "succeeded")]
            );
        }
        Ok(())
    }
}
