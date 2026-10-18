use super::*;
use edgeagent_contracts::MessageEnvelope;
use edgeagent_telemetry::{
    EventSpineContext, EventSpineOutcome, EventSpineStage, record_event_spine_operation,
};
use opentelemetry::propagation::Extractor;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tracing_subscriber::{Layer, layer::Context};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<signals::Signal>>>);
impl<S: tracing::Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if let Some(signal) = signals::Signal::read(event)
            && let Ok(mut captured) = self.0.lock()
        {
            captured.push(signal);
        }
    }
}

#[test]
fn proof_endpoint_rejects_remote_or_ambiguous_egress() {
    for key in [
        "OTEL_RESOURCE_ATTRIBUTES",
        "otel_exporter_otlp_headers",
        "OtEl_TrAcEs_ExPoRtEr",
    ] {
        assert!(is_otel_setting(std::ffi::OsStr::new(key)));
    }
    for key in [
        "OTEL",
        "OTHER_SETTING",
        "XOTEL_HEADERS",
        "\u{00e9}TEL_SETTING",
    ] {
        assert!(!is_otel_setting(std::ffi::OsStr::new(key)));
    }
    assert!(metric_stream().is_some());
    assert!(endpoint("http://127.0.0.1:4318").is_ok());
    for value in [
        "http://localhost:4318",
        "https://127.0.0.1:4318",
        "http://127.0.0.1:0",
        "http://0.0.0.0:4318",
        "http://127.0.0.2:4318",
        "http://127.0.0.1:4318/",
        "http://user@127.0.0.1:4318",
        "http://127.0.0.1:4318?target=x",
        "http://[::1]:4318",
    ] {
        assert!(endpoint(value).is_err());
    }
    assert_eq!(
        (QUEUE, BATCH, CARDINALITY, BODY_LIMIT),
        (64, 8, 20, 262_144)
    );
}

#[test]
fn facade_projection_preserves_w3c_state_and_all_twenty_bounded_classifications()
-> Result<(), WorkerError> {
    let mut value: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../fixtures/v1/apply-probe.json"))?;
    value["tracestate"] = "edgeagent=synthetic".into();
    let envelope = MessageEnvelope::from_json(&serde_json::to_vec(&value)?)?;
    let context = EventSpineContext::from_envelope(&envelope);
    assert_eq!(context.trace_state(), Some("edgeagent=synthetic"));
    let capture = Capture::default();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()), || {
        for stage in [
            EventSpineStage::Publication,
            EventSpineStage::Persistence,
            EventSpineStage::Handling,
            EventSpineStage::Acknowledgement,
        ] {
            for outcome in [
                EventSpineOutcome::Succeeded,
                EventSpineOutcome::Duplicate,
                EventSpineOutcome::RetryScheduled,
                EventSpineOutcome::Quarantined,
                EventSpineOutcome::Failed,
            ] {
                record_event_spine_operation(&context, stage, outcome, 1, Duration::from_millis(5));
            }
        }
    });
    let captured = capture.0.lock().map_err(|_| "capture poisoned")?;
    assert_eq!(captured.len(), 20);
    for signal in captured.iter() {
        assert!(signal.classified().is_some());
        assert_eq!(signal.get("tracestate"), Some("edgeagent=synthetic"));
        assert_eq!(signal.get("baggage"), None);
    }
    Ok(())
}

struct SensitiveDebug(Arc<AtomicBool>);
impl std::fmt::Debug for SensitiveDebug {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn unknown_debug_fields_and_invalid_classifications_never_become_exportable()
-> Result<(), WorkerError> {
    let capture = Capture::default();
    let formatted = Arc::new(AtomicBool::new(false));
    let sensitive = SensitiveDebug(formatted.clone());
    tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()), || {
        for duration in [-1.0, f64::NAN, f64::INFINITY, 31.0] {
            tracing::event!(target: "edgeagent.event_spine", tracing::Level::INFO,
                event_name = "edgeagent.event_spine.operation", event_stage = "handling",
                event_outcome = "succeeded", duration_seconds = duration, delivery_attempt = 1_u64,
                error = ?sensitive, payload = "private-payload", baggage = "private-baggage");
        }
        tracing::event!(target: "edgeagent.event_spine", tracing::Level::INFO,
            event_name = "edgeagent.event_spine.operation", event_stage = "arbitrary-stage",
            event_outcome = "succeeded", duration_seconds = 0.1, delivery_attempt = 1_u64);
    });
    let captured = capture.0.lock().map_err(|_| "capture poisoned")?;
    assert_eq!(captured.len(), 5);
    assert!(captured.iter().all(|s| s.classified().is_none()));
    assert!(!formatted.load(Ordering::Relaxed));
    Ok(())
}
