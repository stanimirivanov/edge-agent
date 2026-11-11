//! Wire evidence for the explanation worker, using the shared bounded OTLP receiver.
#[path = "../../../../crates/event-spine-conformance/tests/process/telemetry/receiver.rs"]
mod receiver;
use opentelemetry_proto::tonic::{
    common::v1::{KeyValue, any_value::Value},
    metrics::v1::metric::Data,
};
use receiver::{Receiver, Wire};
use std::collections::BTreeSet;
type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult = Result<(), TestError>;
pub struct Observer(Receiver);
impl Observer {
    pub async fn start(root: &std::path::Path, mode: u8) -> Result<Self, TestError> {
        let receiver = Receiver::start().await?;
        receiver.mode(mode);
        std::fs::write(root.join("otlp-endpoint"), &receiver.endpoint)?;
        Ok(Self(receiver))
    }
    pub fn assert_outcomes(&self, expected: &[&str], accepted: bool) -> TestResult {
        let records = self.0.records()?;
        let mut outcomes = BTreeSet::new();
        let mut metrics = false;
        let mut traces = false;
        for record in records.iter() {
            assert_eq!(record.accepted, accepted);
            assert!(record.bytes.len() <= 262_144);
            match &record.wire {
                Wire::Traces(wire) => {
                    traces = true;
                    for group in &wire.resource_spans {
                        resource(group.resource.as_ref().ok_or("missing resource")?);
                        for scope in &group.scope_spans {
                            assert_eq!(
                                scope.scope.as_ref().ok_or("missing scope")?.name,
                                "edgeagent.research.explanation"
                            );
                        }
                    }
                    for span in wire
                        .resource_spans
                        .iter()
                        .flat_map(|group| &group.scope_spans)
                        .flat_map(|group| &group.spans)
                    {
                        assert_eq!(span.name, "edgeagent.explanation.operation");
                        assert!(span.links.is_empty() && span.events.is_empty());
                        assert_eq!(
                            keys(&span.attributes),
                            BTreeSet::from(["stage", "outcome", "duration_seconds"])
                        );
                        let stage = text(&span.attributes, "stage").ok_or("missing stage")?;
                        let outcome = text(&span.attributes, "outcome").ok_or("missing outcome")?;
                        assert!(
                            edgeagent_telemetry::ExplanationStage::ALL
                                .iter()
                                .any(|value| value.as_str() == stage)
                        );
                        assert!(
                            edgeagent_telemetry::ExplanationOutcome::ALL
                                .iter()
                                .any(|value| value.as_str() == outcome)
                        );
                        if stage == "completion" {
                            outcomes.insert(outcome);
                        }
                    }
                }
                Wire::Metrics(wire) => {
                    metrics = true;
                    for group in &wire.resource_metrics {
                        resource(group.resource.as_ref().ok_or("missing resource")?);
                        for scope in &group.scope_metrics {
                            assert_eq!(
                                scope.scope.as_ref().ok_or("missing scope")?.name,
                                "edgeagent.research.explanation"
                            );
                        }
                    }
                    for metric in wire
                        .resource_metrics
                        .iter()
                        .flat_map(|group| &group.scope_metrics)
                        .flat_map(|group| &group.metrics)
                    {
                        assert!(matches!(
                            metric.name.as_str(),
                            edgeagent_telemetry::EXPLANATION_OPERATIONS
                                | edgeagent_telemetry::EXPLANATION_DURATION
                        ));
                        match &metric.data {
                            Some(Data::Sum(data)) => {
                                for point in &data.data_points {
                                    assert_eq!(
                                        keys(&point.attributes),
                                        BTreeSet::from(["stage", "outcome"])
                                    );
                                }
                            }
                            Some(Data::Histogram(data)) => {
                                for point in &data.data_points {
                                    assert_eq!(
                                        keys(&point.attributes),
                                        BTreeSet::from(["stage", "outcome"])
                                    );
                                }
                            }
                            _ => return Err("unexpected explanation metric".into()),
                        }
                    }
                }
            }
        }
        assert!(metrics && traces, "worker did not export both signal types");
        for outcome in expected {
            assert!(outcomes.contains(outcome), "missing {outcome}");
        }
        Ok(())
    }
    pub async fn close(&mut self) -> TestResult {
        self.0.close().await
    }
}
fn keys(attributes: &[KeyValue]) -> BTreeSet<&str> {
    attributes
        .iter()
        .map(|attribute| attribute.key.as_str())
        .collect()
}
fn text<'a>(attributes: &'a [KeyValue], name: &str) -> Option<&'a str> {
    let Some(Value::StringValue(value)) = attributes
        .iter()
        .find(|attribute| attribute.key == name)?
        .value
        .as_ref()?
        .value
        .as_ref()
    else {
        return None;
    };
    Some(value)
}

fn resource(resource: &opentelemetry_proto::tonic::resource::v1::Resource) {
    assert_eq!(
        keys(&resource.attributes),
        BTreeSet::from(["service.name", "edgeagent.worker.role"])
    );
    assert_eq!(
        text(&resource.attributes, "service.name"),
        Some("edgeagent.research")
    );
    assert_eq!(
        text(&resource.attributes, "edgeagent.worker.role"),
        Some("offline-explanation-worker")
    );
}
