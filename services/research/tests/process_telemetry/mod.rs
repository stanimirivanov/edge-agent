//! Wire and durability evidence from the actual Research process.
#[path = "../../../../crates/event-spine-conformance/tests/process/telemetry/receiver.rs"]
mod receiver;

use super::{Journey, Result, result, submit};
use futures_util::FutureExt;
use opentelemetry_proto::tonic::{
    common::v1::{KeyValue, any_value::Value},
    metrics::v1::metric::Data,
};
use receiver::{Receiver, Record, Wire};
use std::{collections::BTreeSet, panic::AssertUnwindSafe, time::Duration};

type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult = Result;

const ENDPOINT: &str = "EDGEAGENT_RESEARCH_OTLP_ENDPOINT";

#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and the Research process gate"]
async fn research_exports_bounded_signals_and_survives_stalled_collector() -> Result {
    let mut h = Journey::new().await?;
    let tested = AssertUnwindSafe(qualify(&mut h)).catch_unwind().await;
    h.finish(tested).await
}

async fn qualify(h: &mut Journey) -> Result {
    h.seed_history().await?;
    h.start_servers().await?;
    submit(h, 5).await?;
    let mut collector = Receiver::start().await?;
    collector.mode(2);
    run(h, "offline-worker", &collector, true).await?;
    let rejected = collector.records()?.len();
    assert!(rejected > 0, "stalled receiver must see an export attempt");
    assert!(collector.records()?.iter().all(|r| !r.accepted));
    let stream = h.context.get_stream(&h.stream).await?;
    let consumer: async_nats::jetstream::consumer::PullConsumer =
        stream.get_consumer("research").await?;
    assert_eq!(consumer.cached_info().num_ack_pending, 0);
    assert_eq!(consumer.cached_info().delivered.consumer_sequence, 1);

    // The relay has actual outbox work. Ambient OTLP settings alone must
    // still leave it disabled, even with a listening receiver.
    collector.mode(0);
    run(h, "offline-relay", &collector, false).await?;
    assert_eq!(collector.records()?.len(), rejected);
    let original = result(h, "artifact").await?;

    let input: Vec<u8> = sqlx::query_scalar(
        "SELECT envelope FROM edgeagent_message_inbox WHERE consumer_name='research-completion'",
    )
    .fetch_one(&h.research.pool)
    .await?;
    h.context
        .publish(
            edgeagent_contracts::research_acquisition::DECIDED.subject()?,
            input.into(),
        )
        .await?
        .await?;
    // A fresh process recovers the receipt and resumes healthy export.
    run(h, "offline-worker", &collector, true).await?;
    assert_wire(&collector.records()?[rejected..])?;

    let stream = h.context.get_stream(&h.stream).await?;
    let consumer: async_nats::jetstream::consumer::PullConsumer =
        stream.get_consumer("research").await?;
    assert_eq!(consumer.cached_info().num_ack_pending, 0);
    assert_eq!(consumer.cached_info().delivered.consumer_sequence, 2);
    let rows: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT outcome FROM edgeagent_research_completions")
            .fetch_all(&h.research.pool)
            .await?;
    assert_eq!(rows, vec![original]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_outbox")
            .fetch_one(&h.research.pool)
            .await?,
        2
    );
    collector.close().await
}

async fn run(h: &Journey, mode: &str, collector: &Receiver, enabled: bool) -> Result {
    let mut child = h.command("research", mode)?;
    for (key, _) in std::env::vars_os() {
        if key
            .as_encoded_bytes()
            .get(..5)
            .is_some_and(|p| p.eq_ignore_ascii_case(b"OTEL_"))
        {
            child.env_remove(key);
        }
    }
    child.env_remove(ENDPOINT);
    if enabled {
        child.env(ENDPOINT, &collector.endpoint);
    } else {
        child.env("OTEL_EXPORTER_OTLP_ENDPOINT", &collector.endpoint);
    }
    // Includes business work, process shutdown and the SDK's control deadlines.
    let output = tokio::time::timeout(Duration::from_secs(12), child.output()).await??;
    assert!(
        output.status.success(),
        "Research child failed with {}",
        output.status
    );
    Ok(())
}

fn value<'a>(attrs: &'a [KeyValue], key: &str) -> Option<&'a str> {
    match attrs
        .iter()
        .find(|a| a.key == key)?
        .value
        .as_ref()?
        .value
        .as_ref()?
    {
        Value::StringValue(value) => Some(value),
        _ => None,
    }
}

fn assert_wire(records: &[Record]) -> Result {
    let mut stages = BTreeSet::new();
    let mut instruments = BTreeSet::new();
    for record in records {
        assert!(record.accepted);
        for private in [
            b"\"request_id\"".as_slice(),
            b"\"lookback_sessions\"",
            b"postgresql://",
        ] {
            assert!(!record.bytes.windows(private.len()).any(|w| w == private));
        }
        match &record.wire {
            Wire::Traces(wire) => {
                for group in &wire.resource_spans {
                    let attrs = &group
                        .resource
                        .as_ref()
                        .ok_or("missing trace resource")?
                        .attributes;
                    assert_eq!(attrs.len(), 2);
                    assert_eq!(value(attrs, "service.name"), Some("edgeagent.research"));
                    assert_eq!(
                        value(attrs, "edgeagent.worker.role"),
                        Some("offline-worker")
                    );
                    for span in group.scope_spans.iter().flat_map(|s| &s.spans) {
                        assert_eq!(span.name, "edgeagent.event_spine.operation");
                        assert!(span.attributes.iter().all(|a| matches!(
                            a.key.as_str(),
                            "event_stage"
                                | "event_outcome"
                                | "duration_seconds"
                                | "delivery_attempt"
                                | "process.pid"
                                | "message_id"
                                | "message_source"
                                | "message_type"
                                | "correlation_id"
                                | "causation_id"
                                | "transport_message_key"
                        )));
                        assert_eq!(value(&span.attributes, "event_outcome"), Some("succeeded"));
                        stages
                            .insert(value(&span.attributes, "event_stage").ok_or("missing stage")?);
                    }
                }
            }
            Wire::Metrics(wire) => {
                for group in &wire.resource_metrics {
                    let attrs = &group
                        .resource
                        .as_ref()
                        .ok_or("missing metric resource")?
                        .attributes;
                    assert_eq!(attrs.len(), 2);
                    assert_eq!(value(attrs, "service.name"), Some("edgeagent.research"));
                    assert_eq!(
                        value(attrs, "edgeagent.worker.role"),
                        Some("offline-worker")
                    );
                    for metric in group.scope_metrics.iter().flat_map(|s| &s.metrics) {
                        instruments.insert(metric.name.as_str());
                        let attributes: Vec<_> = match metric.data.as_ref() {
                            Some(Data::Sum(sum)) => {
                                sum.data_points.iter().map(|p| &p.attributes).collect()
                            }
                            Some(Data::Histogram(histogram)) => histogram
                                .data_points
                                .iter()
                                .map(|p| &p.attributes)
                                .collect(),
                            _ => return Err("unexpected OTLP instrument".into()),
                        };
                        assert!(!attributes.is_empty());
                        for labels in attributes {
                            assert_eq!(labels.len(), 2);
                            assert!(matches!(
                                value(labels, "stage"),
                                Some("handling" | "acknowledgement")
                            ));
                            assert_eq!(value(labels, "outcome"), Some("succeeded"));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(stages, BTreeSet::from(["acknowledgement", "handling"]));
    assert_eq!(
        instruments,
        BTreeSet::from([
            "edgeagent.event_spine.operation.duration",
            "edgeagent.event_spine.operations",
        ])
    );
    Ok(())
}
