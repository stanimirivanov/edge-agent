use super::super::scenarios::provision_broker;
use super::*;
use edgeagent_event_spine_conformance::{definitions, registry, replay::ReplayPublisherScope};
use edgeagent_messaging::{DeliveryDisposition, MessageConsumer};
use edgeagent_messaging_nats::{
    JetStreamConsumer,
    topology::{Binding, TopologyLimits, TopologyPlan},
};
use std::time::Duration;

fn fanout_plan() -> Result<TopologyPlan, TestError> {
    Ok(TopologyPlan::new(
        registry()?,
        "m02-process",
        &[Binding::new(
            Component::Gateway,
            definitions()[0].message_type(),
        )],
        &[
            Binding::new(Component::AuditProjector, definitions()[1].message_type()),
            Binding::new(Component::Gateway, definitions()[1].message_type()),
        ],
        TopologyLimits::default(),
    )?)
}

#[tokio::test]
#[ignore = "requires isolated EDGEAGENT_POSTGRES_URL and Docker or pinned EDGEAGENT_NATS_SERVER"]
async fn original_route_replay_reaches_all_subscribers_without_exclusive_targeting() -> TestResult {
    let topology = fanout_plan()?;
    broker::with_broker(&topology, |url| async move {
        let topology = fanout_plan()?;
        provision_broker(&url, &topology).await?;
        database::with_database(|producer| {
            Box::pin(async move {
                assert_eq!(
                    producer.worker("producer", false, &url, true).await?,
                    "accepted"
                );
                assert_eq!(
                    producer.worker("relay", false, &url, true).await?,
                    "published"
                );
                assert_eq!(
                    producer.worker("consumer", true, &url, true).await?,
                    "applied"
                );
                assert_eq!(
                    producer.worker("relay", true, &url, true).await?,
                    "published"
                );
                // The audit subscriber owns a separate private schema/operator login,
                // not access to the producer/Research service's state.
                database::with_database(|audit| {
                    Box::pin(async move {
                        let mut audit_consumer =
                            subscriber(&url, &topology, Component::AuditProjector).await?;
                        let mut peer = subscriber(&url, &topology, Component::Gateway).await?;
                        let original =
                            tokio::time::timeout(Duration::from_secs(5), audit_consumer.receive())
                                .await??;
                        let bytes = original.payload().to_vec();
                        let key = original.metadata().message_key().to_owned();
                        let name = consumer_name(Component::AuditProjector);
                        retain(audit, &name, &key, &definitions()[1].subject()?, &bytes).await?;
                        original.settle(DeliveryDisposition::Quarantined).await?;
                        let peer_original =
                            tokio::time::timeout(Duration::from_secs(5), peer.receive()).await??;
                        assert_eq!(peer_original.payload(), bytes);
                        peer_original
                            .settle(DeliveryDisposition::Acknowledge)
                            .await?;
                        assert_eq!(
                            authorize(
                                audit,
                                &url,
                                true,
                                "auditor",
                                &inbound("event-01", &name, &key),
                                true
                            )
                            .await?,
                            "authorized"
                        );
                        assert_eq!(dispatch(audit, &url, "auditor").await?, "replayed");
                        for consumer in [&mut audit_consumer, &mut peer] {
                            let replay =
                                tokio::time::timeout(Duration::from_secs(5), consumer.receive())
                                    .await??;
                            assert_eq!(
                                replay.payload(),
                                bytes,
                                "replay preserves the entire envelope, not only its ID"
                            );
                            replay.settle(DeliveryDisposition::Acknowledge).await?;
                        }
                        let scope =
                            ReplayPublisherScope::new(&topology, Component::AuditProjector)?;
                        broker_denial(
                            &url,
                            &scope,
                            &format!(
                                "$JS.API.STREAM.INFO.{}",
                                topology.streams()[0].config().name
                            ),
                        )
                        .await?;
                        // The replay identity cannot publish another component's
                        // incoming command, even though that stream exists.
                        broker_denial(&url, &scope, &definitions()[0].subject()?).await?;
                        assert_eq!(audit.count(true, "edgeagent_message_quarantine").await?, 1);
                        assert_eq!(audit.count(true, "conformance_replay_receipt").await?, 1);
                        Ok(())
                    })
                })
                .await
            })
        })
        .await
    })
    .await
}

async fn subscriber(
    url: &str,
    plan: &TopologyPlan,
    component: Component,
) -> Result<JetStreamConsumer, TestError> {
    let stream = plan
        .streams()
        .iter()
        .find(|s| s.definition() == definitions()[1])
        .ok_or("event stream missing")?;
    let role = plan
        .runtime_permissions()
        .iter()
        .find(|p| p.component() == Some(component))
        .ok_or("subscriber missing")?;
    let context = broker::context(url, role).await?;
    Ok(JetStreamConsumer::new(
        context
            .get_stream(&stream.config().name)
            .await?
            .get_consumer(&consumer_name(component))
            .await?,
    )
    .await?)
}

async fn broker_denial(url: &str, scope: &ReplayPublisherScope, subject: &str) -> TestResult {
    let (sender, mut events) = tokio::sync::mpsc::channel(4);
    let client = async_nats::ConnectOptions::new()
        .user_and_password(scope.user().into(), "fixture-only".into())
        .custom_inbox_prefix(scope.inbox_prefix())
        .event_callback(move |event| {
            let sender = sender.clone();
            async move {
                if let async_nats::Event::ServerError(error) = event {
                    let _ = sender.try_send(error);
                }
            }
        })
        .connect(url)
        .await?;
    client.publish(subject.to_owned(), "{}".into()).await?;
    client.flush().await?;
    let error = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await?
        .ok_or("permission event absent")?;
    assert!(
        error
            .to_string()
            .to_lowercase()
            .contains("permissions violation")
    );
    assert!(error.to_string().contains(subject));
    Ok(())
}
