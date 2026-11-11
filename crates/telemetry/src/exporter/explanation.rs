//! Strict projection of explanation signals. Unknown fields invalidate the event.
use super::signals::Signals;
use crate::{ExplanationOutcome, ExplanationStage};
use opentelemetry::{
    Context, KeyValue,
    trace::{Span, Tracer},
};
use std::{
    fmt,
    time::{Duration, SystemTime},
};
use tracing::{
    Event,
    field::{Field, Visit},
};

#[derive(Default)]
pub(super) struct Signal {
    name: bool,
    invalid: bool,
    stage: Option<ExplanationStage>,
    outcome: Option<ExplanationOutcome>,
    duration: Option<Duration>,
}
impl Signal {
    pub(super) fn read(event: &Event<'_>) -> Option<Self> {
        if event.metadata().target() != "edgeagent.explanation" {
            return None;
        }
        let mut signal = Self::default();
        event.record(&mut signal);
        (signal.name
            && !signal.invalid
            && signal.stage.is_some()
            && signal.outcome.is_some()
            && signal.duration.is_some())
        .then_some(signal)
    }
}
impl Visit for Signal {
    fn record_debug(&mut self, _: &Field, _: &dyn fmt::Debug) {
        self.invalid = true;
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "event_name" => self.name = value == "edgeagent.explanation.operation",
            "stage" => {
                self.stage = ExplanationStage::ALL
                    .into_iter()
                    .find(|stage| stage.as_str() == value)
            }
            "outcome" => {
                self.outcome = ExplanationOutcome::ALL
                    .into_iter()
                    .find(|outcome| outcome.as_str() == value)
            }
            _ => self.invalid = true,
        }
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        if field.name() == "duration_seconds" && value <= 40.0 {
            self.duration = Duration::try_from_secs_f64(value).ok();
        } else {
            self.invalid = true;
        }
    }
}
pub(super) fn record(signals: &Signals, event: &Event<'_>) {
    let Some(Signal {
        stage: Some(stage),
        outcome: Some(outcome),
        duration: Some(duration),
        ..
    }) = Signal::read(event)
    else {
        return;
    };
    let dimensions = [
        KeyValue::new("stage", stage.as_str()),
        KeyValue::new("outcome", outcome.as_str()),
    ];
    signals.explanation_operations.add(1, &dimensions);
    signals
        .explanation_duration
        .record(duration.as_secs_f64(), &dimensions);
    let mut attributes = dimensions.to_vec();
    attributes.push(KeyValue::new("duration_seconds", duration.as_secs_f64()));
    let end = SystemTime::now();
    let builder = signals
        .tracer
        .span_builder("edgeagent.explanation.operation")
        .with_start_time(end.checked_sub(duration).unwrap_or(end))
        .with_attributes(attributes);
    signals
        .tracer
        .build_with_context(builder, &Context::new())
        .end_with_timestamp(end);
}
