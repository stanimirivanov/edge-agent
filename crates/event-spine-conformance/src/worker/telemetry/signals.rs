//! Closed projection of classified facade events, not a general log exporter.

use opentelemetry::{
    Context, KeyValue,
    metrics::{Counter, Histogram},
    propagation::{Extractor, TextMapPropagator},
    trace::{Link, Span, TraceContextExt, Tracer},
};
use opentelemetry_sdk::{propagation::TraceContextPropagator, trace::SdkTracer};
use std::{
    collections::BTreeMap,
    fmt,
    time::{Duration, SystemTime},
};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, layer::Context as LayerContext};

pub(super) struct Signals {
    pub(super) tracer: SdkTracer,
    pub(super) operations: Counter<u64>,
    pub(super) duration: Histogram<f64>,
}

impl<S: Subscriber> Layer<S> for Signals {
    fn enabled(&self, metadata: &tracing::Metadata<'_>, _: LayerContext<'_, S>) -> bool {
        metadata.target() == "edgeagent.event_spine"
    }

    fn on_event(&self, event: &Event<'_>, _: LayerContext<'_, S>) {
        let Some(signal) = Signal::read(event) else {
            return;
        };
        let Some((stage, outcome, duration, attempt)) = signal.classified() else {
            return;
        };
        let dimensions = [
            KeyValue::new("stage", stage),
            KeyValue::new("outcome", outcome),
        ];
        self.operations.add(1, &dimensions);
        self.duration.record(duration.as_secs_f64(), &dimensions);
        let mut attributes = vec![
            KeyValue::new("event_stage", stage),
            KeyValue::new("event_outcome", outcome),
            KeyValue::new("delivery_attempt", i64::from(attempt)),
            KeyValue::new("duration_seconds", duration.as_secs_f64()),
            KeyValue::new("process.pid", i64::from(std::process::id())),
        ];
        attributes.extend(
            signal
                .identities
                .iter()
                .map(|(key, value)| KeyValue::new(*key, value.clone())),
        );
        let extracted =
            TraceContextPropagator::new().extract_with_context(&Context::new(), &signal);
        let parent = extracted.span();
        let parent = parent.span_context();
        let links = if parent.is_valid() {
            vec![Link::with_context(parent.clone())]
        } else {
            vec![]
        };
        // These are completed-stage observations. The measured monotonic elapsed
        // duration reconstructs an approximate interval, not a distributed SLO.
        let end = SystemTime::now();
        let start = end.checked_sub(duration).unwrap_or(end);
        let builder = self
            .tracer
            .span_builder("edgeagent.event_spine.operation")
            .with_start_time(start)
            .with_attributes(attributes)
            .with_links(links);
        let mut span = self.tracer.build_with_context(builder, &Context::new());
        span.end_with_timestamp(end);
    }
}

#[derive(Default)]
pub(super) struct Signal {
    name: bool,
    stage: Option<&'static str>,
    outcome: Option<&'static str>,
    duration: Option<Duration>,
    attempt: Option<u32>,
    parent: Option<String>,
    state: Option<String>,
    identities: BTreeMap<&'static str, String>,
}

impl Signal {
    pub(super) fn read(event: &Event<'_>) -> Option<Self> {
        if event.metadata().target() != "edgeagent.event_spine" {
            return None;
        }
        let mut signal = Self::default();
        event.record(&mut signal);
        signal.name.then_some(signal)
    }

    pub(super) fn classified(&self) -> Option<(&'static str, &'static str, Duration, u32)> {
        Some((self.stage?, self.outcome?, self.duration?, self.attempt?))
    }
}

impl Visit for Signal {
    fn record_debug(&mut self, _: &Field, _: &dyn fmt::Debug) {
        // Never format unknown fields or an arbitrary error/source chain.
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "delivery_attempt" {
            self.attempt = value.try_into().ok();
        }
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        if field.name() == "duration_seconds" && value <= 30.0 {
            self.duration = Duration::try_from_secs_f64(value).ok();
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "event_name" => self.name = value == "edgeagent.event_spine.operation",
            "event_stage" => {
                self.stage = match value {
                    "publication" => Some("publication"),
                    "persistence" => Some("persistence"),
                    "handling" => Some("handling"),
                    "acknowledgement" => Some("acknowledgement"),
                    _ => None,
                }
            }
            "event_outcome" => {
                self.outcome = match value {
                    "succeeded" => Some("succeeded"),
                    "duplicate" => Some("duplicate"),
                    "retry_scheduled" => Some("retry_scheduled"),
                    "quarantined" => Some("quarantined"),
                    "failed" => Some("failed"),
                    _ => None,
                }
            }
            "trace_parent" if value.len() == 55 => self.parent = Some(value.into()),
            "trace_state" if value.len() <= 512 => self.state = Some(value.into()),
            "message_id"
            | "message_source"
            | "message_type"
            | "correlation_id"
            | "causation_id"
            | "transport_message_key"
                if !value.is_empty() && value.len() <= 512 =>
            {
                self.identities.insert(field.name(), value.into());
            }
            _ => (),
        }
    }
}

impl Extractor for Signal {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "traceparent" => self.parent.as_deref(),
            "tracestate" => self.state.as_deref(),
            _ => None,
        }
    }
    fn keys(&self) -> Vec<&str> {
        vec!["traceparent", "tracestate"]
    }
}
