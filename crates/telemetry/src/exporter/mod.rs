//! Bounded loopback OTLP lifecycle for synchronous process entry points.

mod explanation;
mod signals;
#[cfg(test)]
mod tests;

use crate::{EVENT_SPINE_OPERATION_DURATION_SECONDS, EVENT_SPINE_OPERATIONS_TOTAL};
use opentelemetry::{KeyValue, metrics::MeterProvider, trace::TracerProvider};
use opentelemetry_otlp::{Protocol, RetryPolicy, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider, Stream, Temporality},
    trace::{BatchConfigBuilder, BatchSpanProcessor, Sampler, SdkTracerProvider},
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tracing_subscriber::prelude::*;

type ExportError = Box<dyn std::error::Error + Send + Sync>;
const EXPORT_TIMEOUT: Duration = Duration::from_millis(250);
const QUEUE: usize = 64;
const BATCH: usize = 8;
const CARDINALITY: usize = 20; // Four stages times five outcomes, never IDs.
const EXPLANATION_CARDINALITY: usize =
    crate::ExplanationStage::ALL.len() * crate::ExplanationOutcome::ALL.len();
const BODY_LIMIT: usize = 262_144;
const BOUNDARIES: [f64; 6] = [0.001, 0.01, 0.1, 1.0, 5.0, 30.0];

/// Providers owned by the process root until application work finishes.
#[must_use]
pub struct Telemetry {
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

fn endpoint(value: &str) -> Result<&str, ExportError> {
    // No DNS, credentials, query, fragment, path, proxy or remote egress.
    let socket: SocketAddr = value
        .strip_prefix("http://")
        .ok_or("invalid OTLP endpoint")?
        .parse()?;
    if socket.ip() != std::net::Ipv4Addr::LOCALHOST || socket.port() == 0 {
        return Err("OTLP endpoint must use an explicit IPv4 loopback port".into());
    }
    Ok(value)
}

impl Telemetry {
    /// Install the allowlisted subscriber before entering the application runtime.
    /// Returns `None` when the process-specific setting is absent.
    ///
    /// # Errors
    /// Rejects invalid settings, ambient OTEL overrides or a conflicting subscriber.
    pub fn start(
        setting: &str,
        instrumentation_scope: &'static str,
        role: &'static str,
        component: &'static str,
    ) -> Result<Option<Self>, ExportError> {
        let Some(value) = std::env::var_os(setting) else {
            return Ok(None);
        };
        let value = value.to_str().ok_or("invalid exporter configuration")?;
        let endpoint = endpoint(value)?;
        if std::env::vars_os().any(|(key, _)| is_otel_setting(&key)) {
            return Err("ambient OTEL configuration rejected".into());
        }
        // Construct blocking HTTP transport outside the application's Tokio runtime.
        // Reqwest may initialize TLS even for the loopback HTTP endpoint.
        // Supply the pinned provider locally without process-wide TLS state.
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
        let client = reqwest::blocking::Client::builder()
            .tls_backend_preconfigured(tls)
            .tls_sslkeylogfile(false)
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
            return Err("OTLP metric view configuration rejected".into());
        }
        let metrics = SdkMeterProvider::builder()
            .with_resource(resource)
            .with_reader(
                PeriodicReader::builder(metric_exporter)
                    .with_interval(Duration::from_secs(1))
                    .build(),
            )
            .with_view(|instrument| {
                if matches!(
                    instrument.name(),
                    crate::EXPLANATION_OPERATIONS | crate::EXPLANATION_DURATION
                ) {
                    Stream::builder()
                        .with_cardinality_limit(EXPLANATION_CARDINALITY)
                        .build()
                        .ok()
                } else {
                    metric_stream()
                }
            })
            .build();
        let meter = metrics.meter(instrumentation_scope);
        let subscriber = tracing_subscriber::registry().with(signals::Signals {
            tracer: traces.tracer(instrumentation_scope),
            operations: meter.u64_counter(EVENT_SPINE_OPERATIONS_TOTAL).build(),
            explanation_operations: meter.u64_counter(crate::EXPLANATION_OPERATIONS).build(),
            explanation_duration: meter
                .f64_histogram(crate::EXPLANATION_DURATION)
                .with_unit("s")
                .with_boundaries(BOUNDARIES.to_vec())
                .build(),
            duration: meter
                .f64_histogram(EVENT_SPINE_OPERATION_DURATION_SECONDS)
                .with_unit("s")
                .with_boundaries(BOUNDARIES.to_vec())
                .build(),
        });
        let telemetry = Self { traces, metrics };
        if tracing::subscriber::set_global_default(subscriber).is_err() {
            telemetry.shutdown();
            return Err("telemetry subscriber already installed".into());
        }
        Ok(Some(telemetry))
    }

    /// Flush best-effort diagnostics after the application runtime has stopped.
    pub fn shutdown(self) {
        // SDK 0.33's meter provider ignores its public timeout argument and its
        // reader uses a five-second control deadline. Bound actual HTTP I/O too;
        // install no observable callbacks. Trace shutdown adds at most one second.
        let traces = self.traces.shutdown_with_timeout(Duration::from_secs(1));
        let metrics = self.metrics.shutdown();
        if traces.is_err() || metrics.is_err() {
            eprintln!("event-spine telemetry flush incomplete");
        }
    }
}
