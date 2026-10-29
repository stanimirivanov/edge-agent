//! Finite proof exporter wiring. SDK types never enter the signal facade.

mod signals;
#[cfg(test)]
mod tests;

use super::WorkerError;
use edgeagent_telemetry::{EVENT_SPINE_OPERATION_DURATION_SECONDS, EVENT_SPINE_OPERATIONS_TOTAL};
use opentelemetry::{KeyValue, metrics::MeterProvider, trace::TracerProvider};
use opentelemetry_otlp::{Protocol, RetryPolicy, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider, Stream, Temporality},
    trace::{BatchConfigBuilder, BatchSpanProcessor, Sampler, SdkTracerProvider},
};
use std::{net::SocketAddr, time::Duration};
use tracing_subscriber::prelude::*;

pub(crate) const ENDPOINT: &str = "EDGEAGENT_PROOF_OTLP_ENDPOINT";
const EXPORT_TIMEOUT: Duration = Duration::from_millis(250);
const QUEUE: usize = 64;
const BATCH: usize = 8;
const CARDINALITY: usize = 20; // Four stages times five outcomes, never IDs.
const BODY_LIMIT: usize = 262_144;
const BOUNDARIES: [f64; 6] = [0.001, 0.01, 0.1, 1.0, 5.0, 30.0];

pub(crate) struct Telemetry {
    traces: SdkTracerProvider,
    metrics: SdkMeterProvider,
}

fn metric_stream() -> Option<Stream> {
    Stream::builder()
        .with_cardinality_limit(CARDINALITY)
        .build()
        .ok()
}

fn is_otel_setting(key: &std::ffi::OsStr) -> bool {
    // Match the ASCII prefix conservatively without requiring the rest
    // of an OS-native name to be Unicode.
    key.as_encoded_bytes()
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"OTEL_"))
}

pub(crate) fn endpoint(value: &str) -> Result<&str, WorkerError> {
    // No DNS, credentials, query, fragment, path, proxy or remote egress.
    let socket: SocketAddr = value
        .strip_prefix("http://")
        .ok_or("invalid proof exporter")?
        .parse()?;
    if socket.ip() != std::net::Ipv4Addr::LOCALHOST || socket.port() == 0 {
        return Err("proof exporter must use an explicit IPv4 loopback port".into());
    }
    Ok(value)
}

impl Telemetry {
    pub(crate) fn start(
        role: &'static str,
        component: &'static str,
    ) -> Result<Option<Self>, WorkerError> {
        let Some(value) = std::env::var_os(ENDPOINT) else {
            return Ok(None);
        };
        let value = value.to_str().ok_or("invalid exporter configuration")?;
        let endpoint = endpoint(value)?;
        if std::env::vars_os().any(|(key, _)| is_otel_setting(&key)) {
            return Err("ambient OTEL configuration is not accepted by proof workers".into());
        }
        // Construct blocking HTTP transport outside the application's Tokio runtime.
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(EXPORT_TIMEOUT)
            .connect_timeout(EXPORT_TIMEOUT)
            .pool_max_idle_per_host(1)
            .build()?;
        let spans = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_http_client(client.clone())
            .with_endpoint(format!("{endpoint}/v1/traces"))
            .with_protocol(Protocol::HttpBinary)
            .with_retry_policy(RetryPolicy::disabled())
            .with_max_request_body_size(BODY_LIMIT)
            .with_timeout(EXPORT_TIMEOUT)
            .build()?;
        let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_http_client(client)
            .with_endpoint(format!("{endpoint}/v1/metrics"))
            .with_protocol(Protocol::HttpBinary)
            .with_temporality(Temporality::Delta)
            .with_retry_policy(RetryPolicy::disabled())
            .with_max_request_body_size(BODY_LIMIT)
            .with_timeout(EXPORT_TIMEOUT)
            .build()?;
        // Empty builder disables environment, host and process resource detectors.
        // PID is trace-only; ephemeral workers must not create metric label series.
        let resource = Resource::builder_empty()
            .with_attributes([
                KeyValue::new("service.name", component),
                KeyValue::new("edgeagent.worker.role", role),
            ])
            .build();
        let processor = BatchSpanProcessor::builder(spans)
            .with_batch_config(
                BatchConfigBuilder::default()
                    .with_max_queue_size(QUEUE)
                    .with_max_export_batch_size(BATCH)
                    .with_scheduled_delay(Duration::from_secs(1))
                    .build(),
            )
            .build();
        let traces = SdkTracerProvider::builder()
            .with_span_processor(processor)
            .with_sampler(Sampler::AlwaysOn)
            .with_resource(resource.clone())
            .with_max_attributes_per_span(16)
            .with_max_links_per_span(1)
            .with_max_events_per_span(0)
            .with_max_attributes_per_link(0)
            .build();
        // Stream is not Clone and its SDK builder returns a non-Send error.
        // Preflight these fixed controls, then create the same view per instrument.
        if metric_stream().is_none() {
            return Err("proof metric view configuration rejected".into());
        }
        let metrics = SdkMeterProvider::builder()
            .with_resource(resource)
            .with_reader(
                PeriodicReader::builder(metric_exporter)
                    .with_interval(Duration::from_secs(1))
                    .build(),
            )
            .with_view(|_| metric_stream())
            .build();
        let meter = metrics.meter("edgeagent.conformance.event-spine");
        let subscriber = tracing_subscriber::registry().with(signals::Signals {
            tracer: traces.tracer("edgeagent.conformance.event-spine"),
            operations: meter.u64_counter(EVENT_SPINE_OPERATIONS_TOTAL).build(),
            duration: meter
                .f64_histogram(EVENT_SPINE_OPERATION_DURATION_SECONDS)
                .with_unit("s")
                .with_boundaries(BOUNDARIES.to_vec())
                .build(),
        });
        let telemetry = Self { traces, metrics };
        if tracing::subscriber::set_global_default(subscriber).is_err() {
            telemetry.shutdown();
            return Err("proof telemetry subscriber already installed".into());
        }
        Ok(Some(telemetry))
    }

    pub(crate) fn shutdown(self) {
        // SDK 0.33's meter provider ignores its public timeout argument and its
        // reader uses a five-second control deadline. Bound actual HTTP I/O too;
        // install no observable callbacks. Trace shutdown adds at most one second.
        let traces = self.traces.shutdown_with_timeout(Duration::from_secs(1));
        let metrics = self.metrics.shutdown();
        if traces.is_err() || metrics.is_err() {
            eprintln!("conformance telemetry flush incomplete; durable result unchanged");
        }
    }
}
