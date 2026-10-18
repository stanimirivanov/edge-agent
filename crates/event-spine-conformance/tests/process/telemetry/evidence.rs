use super::{
    PRIVATE_BAGGAGE, PRIVATE_KEY, PRIVATE_PAYLOAD, STATE, TestError, TestResult,
    receiver::{Record, Wire},
};
use opentelemetry_proto::tonic::{
    common::v1::{KeyValue, any_value::Value},
    metrics::v1::{metric::Data, number_data_point},
};
use std::collections::{BTreeMap, BTreeSet};

fn strings(attributes: &[KeyValue]) -> BTreeMap<&str, &str> {
    attributes
        .iter()
        .filter_map(|a| match a.value.as_ref()?.value.as_ref()? {
            Value::StringValue(value) => Some((a.key.as_str(), value.as_str())),
            _ => None,
        })
        .collect()
}

fn dimensions(attributes: &[KeyValue]) -> Result<(String, String), TestError> {
    let labels = strings(attributes);
    assert_eq!(attributes.len(), 2, "no unreviewed metric dimensions");
    assert_eq!(
        labels.keys().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from(["stage", "outcome"])
    );
    Ok((
        labels.get("stage").ok_or("stage absent")?.to_string(),
        labels.get("outcome").ok_or("outcome absent")?.to_string(),
    ))
}

fn resource(attributes: &[KeyValue]) -> Result<(String, String), TestError> {
    let resource = strings(attributes);
    assert_eq!(
        attributes.len(),
        2,
        "no environment/host/PID metric resource labels"
    );
    assert_eq!(
        resource.keys().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from(["service.name", "edgeagent.worker.role"])
    );
    Ok((
        resource
            .get("service.name")
            .ok_or("component absent")?
            .to_string(),
        resource
            .get("edgeagent.worker.role")
            .ok_or("role absent")?
            .to_string(),
    ))
}

pub fn healthy(records: &[Record], expected_pids: &BTreeSet<i64>) -> TestResult {
    assert!(
        !records.is_empty(),
        "listening collector alone is not evidence"
    );
    let mut spans = BTreeMap::<(String, String), u64>::new();
    let mut counts = BTreeMap::<(String, String), u64>::new();
    let mut durations = BTreeMap::<(String, String), u64>::new();
    let mut span_seconds = BTreeMap::<(String, String), f64>::new();
    let mut metric_seconds = BTreeMap::<(String, String), f64>::new();
    let mut pids = BTreeSet::new();
    let mut trace_resources = BTreeSet::new();
    let mut metric_resources = BTreeSet::new();
    let mut facts = 0;
    let mut anonymous = 0;
    for record in records {
        assert!(
            record.accepted,
            "successful-export proof cannot borrow rejected requests"
        );
        for private in [
            PRIVATE_KEY.as_bytes(),
            PRIVATE_BAGGAGE.as_bytes(),
            PRIVATE_PAYLOAD,
        ] {
            assert!(
                !record.bytes.windows(private.len()).any(|v| v == private),
                "private sentinel entered the OTLP wire"
            );
        }
        match &record.wire {
            Wire::Traces(request) => {
                for rs in &request.resource_spans {
                    trace_resources.insert(resource(
                        &rs.resource
                            .as_ref()
                            .ok_or("trace resource absent")?
                            .attributes,
                    )?);
                    for scope in &rs.scope_spans {
                        for span in &scope.spans {
                            assert_eq!(span.name, "edgeagent.event_spine.operation");
                            assert!(span.end_time_unix_nano >= span.start_time_unix_nano);
                            let attributes = strings(&span.attributes);
                            let allowed = [
                                "event_stage",
                                "event_outcome",
                                "delivery_attempt",
                                "duration_seconds",
                                "process.pid",
                                "message_id",
                                "message_source",
                                "message_type",
                                "correlation_id",
                                "causation_id",
                                "transport_message_key",
                            ];
                            assert!(
                                span.attributes
                                    .iter()
                                    .all(|a| allowed.contains(&a.key.as_str()))
                            );
                            let stage = attributes
                                .get("event_stage")
                                .ok_or("trace stage absent")?
                                .to_string();
                            let outcome = attributes
                                .get("event_outcome")
                                .ok_or("trace outcome absent")?
                                .to_string();
                            let classification = (stage, outcome);
                            *spans.entry(classification.clone()).or_default() += 1;
                            let elapsed = span
                                .attributes
                                .iter()
                                .find(|a| a.key == "duration_seconds")
                                .and_then(|a| a.value.as_ref())
                                .and_then(|a| a.value.as_ref());
                            let Some(Value::DoubleValue(seconds)) = elapsed else {
                                return Err("trace duration absent".into());
                            };
                            assert!(seconds.is_finite() && *seconds >= 0.0 && *seconds <= 30.0);
                            *span_seconds.entry(classification).or_default() += seconds;
                            let pid = span
                                .attributes
                                .iter()
                                .find(|a| a.key == "process.pid")
                                .and_then(|a| a.value.as_ref())
                                .and_then(|a| a.value.as_ref());
                            let Some(Value::IntValue(pid)) = pid else {
                                return Err("trace process identity absent".into());
                            };
                            pids.insert(*pid);
                            if attributes.contains_key("message_id") {
                                assert_eq!(
                                    attributes.get("correlation_id").copied(),
                                    Some("flow-telemetry")
                                );
                                assert_eq!(span.links.len(), 1);
                                let link = span.links.first().ok_or("W3C link absent")?;
                                assert_eq!(link.trace_id, vec![0x11; 16]);
                                assert_eq!(link.span_id, vec![0x22; 8]);
                                assert_eq!(link.trace_state, STATE);
                                assert_eq!(link.flags & 1, 1);
                                if attributes
                                    .get("message_id")
                                    .is_some_and(|id| id.starts_with("probe-applied-"))
                                {
                                    assert_eq!(
                                        attributes.get("causation_id").copied(),
                                        Some("telemetry-good")
                                    );
                                    facts += 1;
                                }
                            } else {
                                assert!(
                                    span.links.is_empty(),
                                    "malformed evidence gets no guessed parent"
                                );
                                assert!(!attributes.contains_key("correlation_id"));
                                anonymous += 1;
                            }
                        }
                    }
                }
            }
            Wire::Metrics(request) => {
                for rm in &request.resource_metrics {
                    metric_resources.insert(resource(
                        &rm.resource
                            .as_ref()
                            .ok_or("metric resource absent")?
                            .attributes,
                    )?);
                    for scope in &rm.scope_metrics {
                        for metric in &scope.metrics {
                            match metric.data.as_ref().ok_or("metric data absent")? {
                                Data::Sum(sum) => {
                                    assert_eq!(metric.name, "edgeagent.event_spine.operations");
                                    assert!(sum.is_monotonic);
                                    assert_eq!(sum.aggregation_temporality, 1);
                                    for point in &sum.data_points {
                                        let Some(number_data_point::Value::AsInt(value)) =
                                            point.value
                                        else {
                                            return Err("counter is not integral".into());
                                        };
                                        *counts
                                            .entry(dimensions(&point.attributes)?)
                                            .or_default() += u64::try_from(value)?;
                                    }
                                }
                                Data::Histogram(histogram) => {
                                    assert_eq!(
                                        metric.name,
                                        "edgeagent.event_spine.operation.duration"
                                    );
                                    assert_eq!(metric.unit, "s");
                                    assert_eq!(histogram.aggregation_temporality, 1);
                                    for point in &histogram.data_points {
                                        let classification = dimensions(&point.attributes)?;
                                        *durations.entry(classification.clone()).or_default() +=
                                            point.count;
                                        let sum = point.sum.ok_or("duration sum absent")?;
                                        assert!(sum.is_finite() && sum >= 0.0);
                                        *metric_seconds.entry(classification).or_default() += sum;
                                        assert_eq!(
                                            point.explicit_bounds,
                                            [0.001, 0.01, 0.1, 1.0, 5.0, 30.0]
                                        );
                                        assert_eq!(point.bucket_counts.len(), 7);
                                        assert_eq!(
                                            point.bucket_counts.iter().sum::<u64>(),
                                            point.count
                                        );
                                    }
                                }
                                _ => return Err("unreviewed metric kind".into()),
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        &pids, expected_pids,
        "wire process identities match the actual child PIDs"
    );
    assert!(pids.len() >= 3);
    assert_eq!(trace_resources, metric_resources);
    assert_eq!(
        trace_resources
            .iter()
            .map(|(_, role)| role.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["producer", "relay", "consumer"])
    );
    assert!(facts > 0 && anonymous > 0);
    counts.retain(|_, count| *count != 0);
    durations.retain(|_, count| *count != 0);
    assert_eq!(
        counts, spans,
        "exported counters reflect classified trace observations exactly"
    );
    assert_eq!(
        durations, spans,
        "duration sample counts agree with the same operations"
    );
    assert_eq!(metric_seconds.len(), span_seconds.len());
    for (classification, expected) in span_seconds {
        let actual = metric_seconds
            .get(&classification)
            .ok_or("duration series absent")?;
        // The SDK may accumulate batches in a different floating-point order.
        assert!(
            (actual - expected).abs() <= 1e-9 * (1.0 + expected),
            "duration sums must measure the same classified spans"
        );
    }
    for classification in [
        ("publication", "succeeded"),
        ("persistence", "failed"),
        ("handling", "duplicate"),
        ("acknowledgement", "retry_scheduled"),
        ("acknowledgement", "quarantined"),
    ] {
        assert!(spans.contains_key(&(classification.0.into(), classification.1.into())));
    }
    Ok(())
}
