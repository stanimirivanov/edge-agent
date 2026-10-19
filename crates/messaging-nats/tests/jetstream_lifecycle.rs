//! Receive ownership and lifecycle proofs against an exclusive local broker.

pub mod support;

use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, pull};
use edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES;
use edgeagent_messaging::{ConsumeErrorKind, DeliveryDisposition, MessageConsumer};
use edgeagent_messaging_nats::JetStreamConsumer;
use futures_util::{FutureExt, StreamExt};
use std::error::Error;
use std::time::Duration;
use support::{
    CASE_DEADLINE, DEADLINE, TestError, TestResult, connect, consumer_config, durable, resource,
    stream_config, with_stream_deadline as with_stream,
};

// The pinned SDK's 30-second expiry plus five-second renewal margin is observed
// explicitly; no arbitrary scheduling sleep coordinates these proofs.
const EXPIRY_OBSERVATION: Duration = Duration::from_secs(37);
const POISON: &str = "edgeagent.command.execution.lifecycle-poison.v1";
const CONTROL: &str = "edgeagent.command.execution.lifecycle-control.v1";
const CLOSED: &str = "edgeagent.command.execution.lifecycle-closed.v1";
const BOUNDED: &str = "edgeagent.command.execution.lifecycle-bounded.v1";

async fn observe_pull(observer: &mut async_nats::Subscriber, phase: &'static str) -> TestResult {
    tokio::time::timeout(DEADLINE, observer.next())
        .await
        .map_err(|_| format!("timed out waiting for {phase}"))?
        .ok_or("pull observer closed")?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn protocol_halt_stops_expiry_and_reconnect_pulls() -> TestResult {
    let observer_context = connect().await?;
    let observer_client = observer_context.client();
    let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(16);
    let client = tokio::time::timeout(
        DEADLINE,
        async_nats::ConnectOptions::new()
            .event_callback(move |event| {
                let sender = events_tx.clone();
                async move {
                    let _ = sender.try_send(event);
                }
            })
            .connect(std::env::var("EDGEAGENT_NATS_URL")?),
    )
    .await??;
    // Consume initial connection event before the reconnect stimulus.
    wait_connected(&mut events_rx, "initial connection").await?;
    let context = jetstream::new(client.clone());
    with_stream(&context, stream_config(&[POISON, CONTROL]), CASE_DEADLINE, |stream| async move {
        let name = resource("halted");
        let poisoned = durable(&stream, &name, POISON).await?;
        let inspector = poisoned.clone();
        let mut observer = observer_client.subscribe(format!("$JS.API.CONSUMER.MSG.NEXT.{}.{}", stream.cached_info().config.name, name)).await?;
        observer_client.flush().await?;
        let mut halted = JetStreamConsumer::new(poisoned).await?;
        context_publish(&client, POISON, vec![b'x'; MAX_PORTABLE_MESSAGE_BYTES + 1]).await?;
        let error = tokio::time::timeout(DEADLINE, halted.receive()).await?.err().ok_or("oversized delivery must halt")?;
        assert_eq!(error.kind(), ConsumeErrorKind::Protocol);
        observe_pull(&mut observer, "poison delivery pull").await?;
        // Keep the adapter alive and do not poll it during the expiry window.
        // Retaining the old SDK stream issues another observable pull here.
        assert!(tokio::time::timeout(EXPIRY_OBSERVATION, observer.next()).await.is_err(), "halted worker issued an expiry pull");

        let control_name = resource("active_control");
        let control = durable(&stream, &control_name, CONTROL).await?;
        let mut active = JetStreamConsumer::new(control).await?;
        let mut control_observer = observer_client.subscribe(format!("$JS.API.CONSUMER.MSG.NEXT.{}.{}", stream.cached_info().config.name, control_name)).await?;
        observer_client.flush().await?;
        // Observe explicit demand, then cancel it before forcing the reconnect.
        // The SDK uses a coalescing watch channel for connection state: an
        // in-flight stream is not guaranteed to see a distinct Disconnected
        // state and issue an automatic reconnect pull within DEADLINE.
        tokio::select! {
            result = active.receive() => return Err(format!("empty active control unexpectedly completed: {:?}", result.err()).into()),
            result = observe_pull(&mut control_observer, "active control initial pull") => result?,
        }
        client.force_reconnect().await?;
        wait_connected(&mut events_rx, "forced reconnect").await?;
        let reconnect_proof = async {
            // Fresh explicit demand on the SAME reconnected client proves
            // transport and observation are live, without asserting an SDK
            // background-renewal heuristic. The halted adapter stays unpolled.
            observe_pull(&mut control_observer, "active control reconnect pull").await?;
            assert!(tokio::time::timeout(Duration::from_secs(1), observer.next()).await.is_err(), "halted worker issued a reconnect pull");
            Ok::<(), TestError>(())
        };
        tokio::select! {
            result = active.receive() => return Err(format!("empty active control unexpectedly completed: {:?}", result.err()).into()),
            result = reconnect_proof => result?,
        }
        let halted_error = halted.receive().now_or_never().ok_or("halted intake must resolve without I/O")?.err().ok_or("halted intake must fail")?;
        assert_eq!(halted_error.kind(), ConsumeErrorKind::Protocol);
        let info = inspector.get_info().await?;
        assert_eq!(info.num_ack_pending, 1);
        assert_eq!(info.delivered.consumer_sequence, 1);
        assert_eq!(stream.get_info().await?.state.messages, 1);
        Ok(())
    }).await
}

async fn wait_connected(
    events: &mut tokio::sync::mpsc::Receiver<async_nats::Event>,
    phase: &'static str,
) -> TestResult {
    tokio::time::timeout(DEADLINE, async {
        loop {
            if matches!(
                events.recv().await.ok_or("connection events closed")?,
                async_nats::Event::Connected
            ) {
                return Ok::<(), TestError>(());
            }
        }
    })
    .await
    .map_err(|_| format!("timed out waiting for {phase}"))?
}

async fn context_publish(
    client: &async_nats::Client,
    subject: &'static str,
    payload: Vec<u8>,
) -> TestResult {
    jetstream::new(client.clone())
        .publish(subject, payload.into())
        .await?
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn deleted_consumer_closes_intake_while_adapter_remains_alive() -> TestResult {
    let context = connect().await?;
    let client = context.client();
    with_stream(&context, stream_config(&[CLOSED]), CASE_DEADLINE, |stream| async move {
        let name = resource("closed");
        let configured = stream.create_consumer(pull::Config { ack_wait: Duration::from_secs(1), ..consumer_config(&name, CLOSED) }).await?;
        let inspector = configured.clone();
        let mut observer = client
            .subscribe(format!(
                "$JS.API.CONSUMER.MSG.NEXT.{}.{}",
                stream.cached_info().config.name,
                name
            ))
            .await?;
        client.flush().await?;
        let mut consumer = JetStreamConsumer::new(configured).await?;
        {
            let waiting = consumer.receive();
            tokio::pin!(waiting);
            tokio::select! {
                result = &mut waiting => return Err(format!("empty receive unexpectedly completed: {:?}", result.err()).into()),
                result = observe_pull(&mut observer, "cancelled receive pull") => result?,
            }
            // Cancel a polled receive, not the adapter. The outstanding broker
            // request can still deliver; cancellation does not prove non-delivery.
            // Recovery can wait for ack-wait and increase the attempt count.
        }
        jetstream::new(client.clone()).publish(CLOSED, "bounded synthetic payload".into()).await?.await?;
        let recovered = consumer.receive().await?;
        assert_eq!(recovered.payload(), b"bounded synthetic payload");
        assert!(recovered.metadata().delivery_attempt() >= 1);
        recovered.settle(DeliveryDisposition::Acknowledge).await?;
        // Recovery owns a new stream; drain its observed pull before using the
        // next request as the deletion barrier.
        observe_pull(&mut observer, "recovered receive pull").await?;
        let deletion = async {
            observe_pull(&mut observer, "consumer deletion pull").await?;
            // A Core subscription can observe the request before JetStream
            // registers it. Wait for broker state, not a scheduling sleep.
            tokio::time::timeout(DEADLINE, async {
                while inspector.get_info().await?.num_waiting == 0 {
                    tokio::task::yield_now().await;
                }
                Ok::<(), TestError>(())
            })
            .await??;
            assert!(stream.delete_consumer(&name).await?.success);
            Ok::<(), TestError>(())
        };
        let (received, deleted) = tokio::join!(consumer.receive(), deletion);
        deleted?;
        let error = received.err().ok_or("deleted consumer must close intake")?;
        assert_eq!(error.kind(), ConsumeErrorKind::Unavailable);
        let cause = error
            .source()
            .and_then(|source| source.downcast_ref::<pull::MessagesError>())
            .ok_or("SDK cause")?;
        assert_eq!(cause.kind(), pull::MessagesErrorKind::ConsumerDeleted);
        for _ in 0..2 {
            let error = consumer
                .receive()
                .now_or_never()
                .ok_or("closed intake must resolve without I/O")?
                .err()
                .ok_or("closed intake must fail")?;
            assert_eq!(error.kind(), ConsumeErrorKind::Unavailable);
            assert!(error.source().is_none());
        }
        client.flush().await?;
        // A cached terminated iterator can return immediately yet retain its
        // background worker. Keep this handle alive past the renewal deadline.
        assert!(tokio::time::timeout(EXPIRY_OBSERVATION, observer.next()).await.is_err(), "closed worker issued an expiry pull");
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_NATS_URL and an isolated NATS JetStream instance"]
async fn completed_receive_does_not_prefetch_while_delivery_is_held() -> TestResult {
    let context = connect().await?;
    let client = context.client();
    with_stream(
        &context,
        stream_config(&[BOUNDED]),
        CASE_DEADLINE,
        |stream| async move {
            let name = resource("bounded");
            let configured = stream
                .create_consumer(pull::Config {
                    durable_name: Some(name.clone()),
                    ack_policy: AckPolicy::Explicit,
                    filter_subject: BOUNDED.to_owned(),
                    ack_wait: Duration::from_secs(120),
                    max_ack_pending: 8,
                    max_batch: 8,
                    ..pull::Config::default()
                })
                .await?;
            let inspector = configured.clone();
            assert_eq!(inspector.cached_info().config.max_ack_pending, 8);
            assert_eq!(inspector.cached_info().config.max_batch, 8);
            let mut observer = client
                .subscribe(format!(
                    "$JS.API.CONSUMER.MSG.NEXT.{}.{}",
                    stream.cached_info().config.name,
                    name
                ))
                .await?;
            client.flush().await?;
            for sequence in 0..4 {
                context_publish(&client, BOUNDED, vec![sequence]).await?;
            }
            let mut consumer = JetStreamConsumer::new(configured).await?;
            let first = consumer.receive().await?;
            assert_eq!(first.payload(), &[0]);
            let request = tokio::time::timeout(DEADLINE, observer.next())
                .await?
                .ok_or("initial pull")?;
            let request: serde_json::Value = serde_json::from_slice(&request.payload)?;
            assert_eq!(request["batch"], 1);
            assert_eq!(request["expires"], 30_000_000_000_u64);
            assert!(
                tokio::time::timeout(EXPIRY_OBSERVATION, observer.next())
                    .await
                    .is_err(),
                "completed receive renewed a pull while the application held its delivery"
            );
            let info = inspector.get_info().await?;
            assert_eq!(info.delivered.consumer_sequence, 1);
            assert_eq!(info.num_ack_pending, 1);
            assert_eq!(info.num_pending, 3);
            first.settle(DeliveryDisposition::Acknowledge).await?;
            for sequence in 1..4 {
                let delivery = consumer.receive().await?;
                assert_eq!(delivery.payload(), &[sequence]);
                assert_eq!(delivery.metadata().delivery_attempt(), 1);
                delivery.settle(DeliveryDisposition::Acknowledge).await?;
            }
            assert_eq!(inspector.get_info().await?.num_ack_pending, 0);
            Ok(())
        },
    )
    .await
}
