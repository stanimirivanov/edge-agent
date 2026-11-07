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

#[tokio::test]
#[ignore = "requires isolated PostgreSQL/NATS and the Research process gate"]
async fn gateway_and_projector_export_without_depending_on_collector_success() -> Result {
    for accepted in [true, false] {
        let mut h = Journey::new().await?;
        let tested = AssertUnwindSafe(qualify_handoff(&mut h, accepted))
            .catch_unwind()
            .await;
        h.finish(tested).await?;
    }
    Ok(())
}

async fn qualify_handoff(h: &mut Journey, accepted: bool) -> Result {
    let mut gateway = Receiver::start().await?;
    let mut projector = Receiver::start().await?;
    gateway.mode(u8::from(!accepted));
    projector.mode(u8::from(!accepted));
    h.seed_history().await?;
    h.start_servers_with_gateway_otlp(Some(&gateway.endpoint))
        .await?;
    submit(h, 5).await?;
    h.run("research", "offline-worker", &[]).await?;
    h.run("research", "offline-relay", &[]).await?;
    for _ in 0..2 {
        let mut command = h.command("projector", "offline-worker")?;
        let output = tokio::time::timeout(
            Duration::from_secs(12),
            command
                .env("EDGEAGENT_PROJECTOR_OTLP_ENDPOINT", &projector.endpoint)
                .output(),
        )
        .await??;
        assert!(
            output.status.success(),
            "projector failed with {}",
            output.status
        );
    }
    let stream = h.context.get_stream(&h.stream).await?;
    let consumer: async_nats::jetstream::consumer::PullConsumer =
        stream.get_consumer("projector").await?;
    assert_eq!(consumer.cached_info().num_ack_pending, 0);
    assert_eq!(consumer.cached_info().delivered.consumer_sequence, 2);
    let retained: Vec<u8> =
        sqlx::query_scalar("SELECT outcome FROM edgeagent_research_completions")
            .fetch_one(&h.research.pool)
            .await?;
    assert_eq!(
        h.request(
            "GET",
            "/v1/research-requests/operator/qualification/outcome",
            &[]
        )
        .await?,
        (200, retained),
    );
    // Gateway is still serving. Wait for the background reader and trace batch.
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            {
                let records = gateway.records()?;
                if records.iter().any(|r| matches!(r.wire, Wire::Traces(_)))
                    && records.iter().any(|r| matches!(r.wire, Wire::Metrics(_)))
                {
                    return Ok::<_, TestError>(());
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| "Gateway did not emit both signal types")??;
    assert_wire(
        &gateway.records()?,
        "edgeagent.gateway",
        "serve",
        &["persistence"],
        accepted,
    )?;
    assert_wire(
        &projector.records()?,
        "edgeagent.audit-projector",
        "offline-worker",
        &["acknowledgement", "handling"],
        accepted,
    )?;
    gateway.close().await?;
    projector.close().await
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
    assert_wire(
        &collector.records()?[rejected..],
        "edgeagent.research",
        "offline-worker",
        &["acknowledgement", "handling"],
        true,
    )?;

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

fn assert_wire(
    records: &[Record],
    service: &str,
    role: &str,
    expected_stages: &[&str],
    accepted: bool,
) -> Result {
    let research = service == "edgeagent.research";
    let valid_outcome = |outcome| {
        outcome == Some("succeeded")
            || (service == "edgeagent.gateway" && outcome == Some("duplicate"))
    };
    let mut stages = BTreeSet::new();
    let mut instruments = BTreeSet::new();
    for record in records {
        assert_eq!(record.accepted, accepted);
        if !research {
            for private in [b"operator".as_slice(), b"qualification"] {
                assert!(!record.bytes.windows(private.len()).any(|w| w == private));
            }
        }
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
                    assert_eq!(value(attrs, "service.name"), Some(service));
                    assert_eq!(value(attrs, "edgeagent.worker.role"), Some(role));
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
                        if !research {
                            assert_eq!(span.attributes.len(), 5);
                            assert!(span.links.is_empty());
                        }
                        assert!(valid_outcome(value(&span.attributes, "event_outcome")));
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
                    assert_eq!(value(attrs, "service.name"), Some(service));
                    assert_eq!(value(attrs, "edgeagent.worker.role"), Some(role));
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
                            assert!(
                                value(labels, "stage")
                                    .is_some_and(|stage| expected_stages.contains(&stage))
                            );
                            assert!(valid_outcome(value(labels, "outcome")));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(stages, expected_stages.iter().copied().collect());
    assert_eq!(
        instruments,
        BTreeSet::from([
            "edgeagent.event_spine.operation.duration",
            "edgeagent.event_spine.operations",
        ])
    );
    Ok(())
}
