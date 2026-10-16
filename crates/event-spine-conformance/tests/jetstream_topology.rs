//! Automatically isolated authenticated broker proofs; default tests stay offline.

#[path = "topology/fixture.rs"]
mod fixture;

use std::{error::Error as _, time::Duration};

use async_nats::{
    ConnectOptions, Event,
    jetstream::{self, consumer::pull},
};
use edgeagent_contracts::{Component, MessageEnvelope, MessageMetadata};
use edgeagent_event_spine_conformance::{ApplyProbe, ProbeApplied, definitions, registry};
use edgeagent_messaging::{
    DeliveryDisposition, MessageConsumer, MessagePublisher, PublishErrorKind, RetryDelay,
};
use edgeagent_messaging_nats::{
    JetStreamConsumer, JetStreamPublisher,
    topology::{
        Binding, SubjectPermissions, TopologyErrorKind, TopologyLimits, TopologyPlan, provision,
    },
};
use fixture::{DEADLINE, TestError, TestResult, context, with_broker};
use futures_util::StreamExt;

fn plan(limits: TopologyLimits) -> Result<TopologyPlan, TestError> {
    Ok(TopologyPlan::new(
        registry()?,
        "m02",
        &[Binding::new(
            Component::Gateway,
            definitions()[0].message_type(),
        )],
        &[
            Binding::new(Component::Gateway, definitions()[1].message_type()),
            Binding::new(Component::AuditProjector, definitions()[1].message_type()),
        ],
        limits,
    )?)
}

fn role(plan: &TopologyPlan, component: Component) -> Result<&SubjectPermissions, TestError> {
    plan.runtime_permissions()
        .iter()
        .find(|role| role.component() == Some(component))
        .ok_or_else(|| "missing runtime role".into())
}

fn metadata(event: bool, id: &str) -> MessageMetadata {
    let definition = definitions()[usize::from(event)];
    MessageMetadata {
        id: id.into(),
        source: if event {
            Component::Research
        } else {
            Component::Gateway
        }
        .source_uri()
        .into(),
        message_type: definition.message_type().into(),
        subject: "probe/probe-01".into(),
        time: "2026-10-16T00:00:00Z".into(),
        data_schema: definition.data_schema().into(),
        correlation_id: "correlation-01".into(),
        causation_id: "request-01".into(),
        idempotency_key: id.into(),
        partition_key: "probe/probe-01".into(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
        trace_state: None,
    }
}

fn command(id: &str) -> Result<MessageEnvelope, TestError> {
    Ok(ApplyProbe::new("probe-01", 0, 42)?.envelope(metadata(false, id))?)
}
fn fact() -> Result<MessageEnvelope, TestError> {
    Ok(ProbeApplied::new("probe-01", 1, 42)?.envelope(metadata(true, "fact-01"))?)
}

async fn consumer(
    context: &jetstream::Context,
    plan: &TopologyPlan,
    event: bool,
    component: Component,
) -> Result<JetStreamConsumer, TestError> {
    let stream = plan
        .streams()
        .iter()
        .find(|stream| stream.definition() == definitions()[usize::from(event)])
        .ok_or("stream absent")?;
    let durable = component.as_str().replace('.', "_");
    Ok(JetStreamConsumer::new(
        context
            .get_stream(&stream.config().name)
            .await?
            .get_consumer::<pull::Config>(&durable)
            .await?,
    )
    .await?)
}

#[test]
fn topology_fixture_uses_the_pinned_local_broker_release() {
    assert!(fixture::pinned_image_matches_local_profile());
}

#[tokio::test]
#[ignore = "requires Docker or EDGEAGENT_NATS_SERVER pointing to pinned nats-server 2.14.7; creates an exclusive authenticated broker"]
async fn provisioning_preserves_evidence_and_rejects_drift_before_writes() -> TestResult {
    let plan = plan(TopologyLimits::default())?;
    let topology = plan.clone();
    with_broker(&plan, |url| async move {
        let control = context(&url, topology.provisioner_permissions()).await?;
        let inspector = jetstream::new(fixture::connect(&url, "inspector", "_INBOX.inspector").await?);
        let command = topology.streams().iter().find(|stream| stream.definition() == definitions()[0]).ok_or("command absent")?;
        let event = topology.streams().iter().find(|stream| stream.definition() == definitions()[1]).ok_or("event absent")?;
        let mut incompatible = event.config().clone(); incompatible.max_bytes += 1;
        inspector.create_stream(incompatible.clone()).await?;
        let retained = fact()?.to_json()?;
        inspector.publish(definitions()[1].subject()?, retained.clone().into()).await?.await?;
        assert_eq!(provision(&control, &topology).await.err().ok_or("drift accepted")?.kind(), TopologyErrorKind::Conflict);
        let absence = inspector.get_stream(&command.config().name).await.err().ok_or("preflight wrote missing stream")?;
        assert!(matches!(absence.kind(), jetstream::context::GetStreamErrorKind::JetStream(error) if error.error_code() == jetstream::ErrorCode::STREAM_NOT_FOUND));
        let unchanged = inspector.get_stream(&event.config().name).await?;
        assert_eq!(unchanged.cached_info().config.max_bytes, incompatible.max_bytes);
        assert_eq!(unchanged.cached_info().state.messages, 1);
        // Deliberate operator repair in the test, never automatic provisioner update.
        inspector.update_stream(event.config().clone()).await?;
        provision(&control, &topology).await?;
        provision(&control, &topology).await?;
        let audit = context(&url, role(&topology, Component::AuditProjector)?).await?;
        let mut reader = consumer(&audit, &topology, true, Component::AuditProjector).await?;
        let delivery = tokio::time::timeout(DEADLINE, reader.receive()).await??;
        assert_eq!(delivery.payload(), retained);
        delivery.settle(DeliveryDisposition::Acknowledge).await?;
        let stream = inspector.get_stream(&command.config().name).await?;
        let mut config = command.consumers()[0].config().clone(); config.max_ack_pending += 1;
        stream.update_consumer(config.clone()).await?;
        assert_eq!(provision(&control, &topology).await.err().ok_or("consumer drift accepted")?.kind(), TopologyErrorKind::Conflict);
        assert_eq!(stream.consumer_info(config.name.as_deref().ok_or("consumer name missing")?).await?.config.max_ack_pending, config.max_ack_pending);
        let event_stream = inspector.get_stream(&event.config().name).await?;
        assert_eq!(event_stream.cached_info().state.messages, 1);
        // Even an undeclared consumer is a conflict, not silently adopted/deleted.
        let mut extra = event.consumers()[0].config().clone(); extra.name = Some("foreign".into()); extra.durable_name = extra.name.clone();
        event_stream.create_consumer_strict(extra).await?;
        stream.update_consumer(command.consumers()[0].config().clone()).await?;
        assert_eq!(provision(&control, &topology).await.err().ok_or("foreign durable accepted")?.kind(), TopologyErrorKind::Conflict);
        assert!(event_stream.consumer_info("foreign").await.is_ok());
        Ok(())
    }).await
}

#[tokio::test]
#[ignore = "requires Docker or EDGEAGENT_NATS_SERVER pointing to pinned nats-server 2.14.7; creates an exclusive authenticated broker"]
async fn runtime_permissions_enforce_delivery_retention_and_admission_bounds() -> TestResult {
    let plan = plan(TopologyLimits::new(2, 1024 * 1024, 1, 1)?)?;
    let topology = plan.clone();
    with_broker(&plan, |url| async move {
        let control = context(&url, topology.provisioner_permissions()).await?;
        provision(&control, &topology).await?;
        let gateway = role(&topology, Component::Gateway)?;
        let research = role(&topology, Component::Research)?;
        let gateway_context = context(&url, gateway).await?;
        let research_context = context(&url, research).await?;
        let publisher = JetStreamPublisher::from_context(gateway_context.clone());
        let mut worker = consumer(&research_context, &topology, false, Component::Research).await?;
        publisher.publish(definitions()[0], &command("command-01")?).await?;
        let delivery = tokio::time::timeout(DEADLINE, worker.receive()).await??;
        let bytes = delivery.payload().to_vec();
        delivery.settle(DeliveryDisposition::RetryAfter(RetryDelay::new(Duration::from_millis(1))?)).await?;
        let redelivery = tokio::time::timeout(DEADLINE, worker.receive()).await??;
        assert_eq!(redelivery.payload(), bytes);
        assert!(redelivery.metadata().delivery_attempt() >= 2);
        redelivery.settle(DeliveryDisposition::Acknowledge).await?;
        publisher.publish(definitions()[0], &command("command-02")?).await?;
        publisher.publish(definitions()[0], &command("command-03")?).await?;
        let capacity = publisher.publish(definitions()[0], &command("command-04")?).await.err().ok_or("capacity admitted extra message")?;
        // The pinned broker wraps storage/capacity errors in generic 10077.
        // Preserve the adapter's conservative unknown-confirmation contract;
        // inspect durable state separately instead of inventing a terminal reject.
        assert_eq!(capacity.kind(), PublishErrorKind::ConfirmationUnknown);
        let cause = capacity.source().and_then(|source| source.source()).and_then(|source| source.downcast_ref::<jetstream::Error>()).ok_or("typed capacity cause absent")?;
        assert_eq!(cause.error_code().0, 10077);
        let command_stream = topology.streams().iter().find(|stream| stream.definition() == definitions()[0]).ok_or("command absent")?;
        let held = tokio::time::timeout(DEADLINE, worker.receive()).await??;
        let stream = research_context.get_stream(&command_stream.config().name).await?;
        assert_eq!(stream.cached_info().state.messages, 2);
        let raw: jetstream::consumer::PullConsumer = stream.get_consumer("edgeagent_research").await?;
        let mut blocked = raw.fetch().max_messages(1).expires(Duration::from_millis(100)).messages().await?;
        assert!(tokio::time::timeout(DEADLINE, blocked.next()).await?.is_none());
        assert_eq!(stream.consumer_info("edgeagent_research").await?.num_ack_pending, 1);
        held.settle(DeliveryDisposition::Acknowledge).await?;
        let remaining = tokio::time::timeout(DEADLINE, worker.receive()).await??;
        remaining.settle(DeliveryDisposition::Acknowledge).await?;
        assert_eq!(research_context.get_stream(&command_stream.config().name).await?.cached_info().state.messages, 0);
        let empty = serde_json::json!({"probe_id":"probe-01","expected_revision":0,"value":42,"padding":""});
        let mut boundary_metadata = metadata(false, &"a".repeat(128));
        boundary_metadata.source = format!("urn:{}", "a".repeat(508));
        let base = definitions()[0].build(boundary_metadata.clone(), &empty)?.to_json()?.len();
        let mut boundary = empty;
        boundary["padding"] = serde_json::json!("x".repeat(262_144 - base));
        let mut oversize_payload = boundary.clone();
        oversize_payload["padding"] = serde_json::json!(format!("{}x", boundary["padding"].as_str().ok_or("padding absent")?));
        let oversized = MessageEnvelope::from_payload(boundary_metadata.clone(), &oversize_payload)?;
        let boundary = definitions()[0].build(boundary_metadata, &boundary)?;
        assert_eq!(boundary.to_json()?.len(), 262_144);
        assert_eq!(oversized.to_json()?.len(), 262_145);
        let oversized_error = publisher.publish(definitions()[0], &oversized).await.err().ok_or("oversized body admitted")?;
        assert_eq!(oversized_error.kind(), PublishErrorKind::Contract);
        assert!(matches!(oversized_error.source().and_then(|source| source.downcast_ref::<edgeagent_contracts::MessageRoutingError>()), Some(edgeagent_contracts::MessageRoutingError::EnvelopeTooLarge { actual_bytes: 262_145, maximum_bytes: 262_144 })));
        publisher.publish(definitions()[0], &boundary).await?;
        JetStreamPublisher::from_context(research_context).publish(definitions()[1], &fact()?).await?;
        for component in [Component::Gateway, Component::AuditProjector] {
            let subscriber = context(&url, role(&topology, component)?).await?;
            let mut reader = consumer(&subscriber, &topology, true, component).await?;
            let delivery = tokio::time::timeout(DEADLINE, reader.receive()).await??;
            assert_eq!(delivery.payload(), fact()?.to_json()?);
            delivery.settle(DeliveryDisposition::Acknowledge).await?;
        }
        let inspector = jetstream::new(fixture::connect(&url, "inspector", "_INBOX.inspector").await?);
        let event = topology.streams().iter().find(|stream| stream.definition() == definitions()[1]).ok_or("event absent")?;
        assert_eq!(inspector.get_stream(&event.config().name).await?.cached_info().state.messages, 1);
        assert!(inspector.publish(definitions()[0].subject()?, vec![b'x'; 263_169].into()).await?.await.is_err());
        for subject in [definitions()[1].subject()?, format!("$JS.API.STREAM.CREATE.{}", command_stream.config().name), format!("$JS.API.STREAM.DELETE.{}", command_stream.config().name), format!("$JS.API.CONSUMER.MSG.NEXT.{}.edgeagent_research", command_stream.config().name), format!("$JS.API.STREAM.MSG.GET.{}", event.config().name)] {
            denied(&url, gateway, &subject, false).await?;
        }
        denied(&url, gateway, &format!("{}.>", topology.provisioner_permissions().inbox_prefix()), true).await?;
        denied(&url, gateway, &definitions()[1].subject()?, true).await?;
        denied(&url, topology.provisioner_permissions(), &definitions()[0].subject()?, false).await?;
        assert_eq!(inspector.get_stream(&event.config().name).await?.cached_info().state.messages, 1);
        Ok(())
    }).await
}

// Observe an explicit broker permission violation, never infer denial from a
// timeout or a failed request that could instead be a missing responder.
async fn denied(
    url: &str,
    role: &SubjectPermissions,
    subject: &str,
    subscription: bool,
) -> TestResult {
    let (sender, mut events) = tokio::sync::mpsc::channel(4);
    let client = ConnectOptions::new()
        .user_and_password(
            role.component()
                .map_or("provisioner", |component| component.as_str())
                .into(),
            "fixture-only".into(),
        )
        .custom_inbox_prefix(role.inbox_prefix())
        .event_callback(move |event| {
            let sender = sender.clone();
            async move {
                if let Event::ServerError(error) = event {
                    let _ = sender.try_send(error);
                }
            }
        })
        .connect(url)
        .await?;
    let _subscriber = if subscription {
        Some(client.subscribe(subject.to_owned()).await?)
    } else {
        client.publish(subject.to_owned(), "{}".into()).await?;
        None
    };
    client.flush().await?;
    let error = tokio::time::timeout(DEADLINE, events.recv())
        .await?
        .ok_or("broker permission event absent")?;
    assert!(
        error
            .to_string()
            .to_lowercase()
            .contains("permissions violation")
    );
    assert!(error.to_string().contains(subject));
    Ok(())
}
