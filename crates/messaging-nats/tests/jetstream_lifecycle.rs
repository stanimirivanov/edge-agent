//! Owned worker lifecycle proofs against an exclusive, isolated JetStream broker.

use async_nats::jetstream;
use async_nats::jetstream::consumer::{AckPolicy, pull};
use async_nats::jetstream::stream::{Config, StorageType};
use edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES;
use edgeagent_messaging::{ConsumeErrorKind, DeliveryDisposition, MessageConsumer};
use edgeagent_messaging_nats::JetStreamConsumer;
use futures_util::{FutureExt, StreamExt};
use std::error::Error;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

type TestError = Box<dyn Error + Send + Sync>;
type TestResult = Result<(), TestError>;
const DEADLINE: Duration = Duration::from_secs(10);
// The pinned SDK renews an unpolled stream after its 30-second expiry plus
// 5 seconds. Observe beyond that deadline, not an arbitrary scheduling sleep.
const EXPIRY_OBSERVATION: Duration = Duration::from_secs(37);
const CASE_DEADLINE: Duration = Duration::from_secs(60);
const POISON: &str = "edgeagent.command.execution.lifecycle-poison.v1";
const CONTROL: &str = "edgeagent.command.execution.lifecycle-control.v1";
const CLOSED: &str = "edgeagent.command.execution.lifecycle-closed.v1";
static NEXT_RESOURCE: AtomicU64 = AtomicU64::new(1);

fn resource(prefix: &str) -> String {
    format!(
        "{prefix}_{}_{}",
        std::process::id(),
        NEXT_RESOURCE.fetch_add(1, Ordering::Relaxed)
    )
}

async fn connect() -> Result<jetstream::Context, TestError> {
    let url = std::env::var("EDGEAGENT_NATS_URL")?;
    let client = tokio::time::timeout(DEADLINE, async_nats::connect(url)).await??;
    Ok(jetstream::ContextBuilder::new()
        .timeout(DEADLINE)
        .build(client))
}

async fn with_stream<F, Fut>(context: &jetstream::Context, subjects: &[&str], body: F) -> TestResult
where
    F: FnOnce(jetstream::stream::Stream) -> Fut,
    Fut: Future<Output = TestResult>,
{
    let name = resource("EDGEAGENT_LIFECYCLE");
    let stream = tokio::time::timeout(
        DEADLINE,
        context.create_stream(Config {
            name: name.clone(),
            subjects: subjects
                .iter()
                .map(|subject| (*subject).to_owned())
                .collect(),
            storage: StorageType::Memory,
            ..Config::default()
        }),
    )
    .await??;
    let outcome =
        AssertUnwindSafe(async { tokio::time::timeout(CASE_DEADLINE, body(stream)).await? })
            .catch_unwind()
            .await;
    let cleanup = async {
        if !tokio::time::timeout(DEADLINE, context.delete_stream(name))
            .await??
            .success
        {
            return Err("owned stream cleanup was not confirmed".into());
        }
        Ok::<(), TestError>(())
    }
    .await;
    match outcome {
        Ok(result) => {
            cleanup?;
            result
        }
        Err(panic) => {
            if cleanup.is_err() {
                eprintln!("owned stream cleanup failed after proof panic");
            }
            std::panic::resume_unwind(panic)
        }
    }
}

async fn durable(
    stream: &jetstream::stream::Stream,
    name: &str,
    subject: &str,
) -> Result<jetstream::consumer::PullConsumer, TestError> {
    Ok(stream
        .create_consumer(pull::Config {
            durable_name: Some(name.to_owned()),
            ack_policy: AckPolicy::Explicit,
            filter_subject: subject.to_owned(),
            // Preserve unsettled evidence throughout the observation window. No
            // max_ack_pending/max_batch restriction masks continued worker pulls.
            ack_wait: Duration::from_secs(120),
            ..pull::Config::default()
        })
        .await?)
}

async fn observe_pull(observer: &mut async_nats::Subscriber) -> TestResult {
    tokio::time::timeout(DEADLINE, observer.next())
        .await?
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
    wait_connected(&mut events_rx).await?;
    let context = jetstream::new(client.clone());
    with_stream(&context, &[POISON, CONTROL], |stream| async move {
        let name = resource("halted");
        let poisoned = durable(&stream, &name, POISON).await?;
        let inspector = poisoned.clone();
        let mut observer = observer_client.subscribe(format!("$JS.API.CONSUMER.MSG.NEXT.{}.{}", stream.cached_info().config.name, name)).await?;
        observer_client.flush().await?;
        let mut halted = JetStreamConsumer::new(poisoned).await?;
        context_publish(&client, POISON, vec![b'x'; MAX_PORTABLE_MESSAGE_BYTES + 1]).await?;
        let error = tokio::time::timeout(DEADLINE, halted.receive()).await?.err().ok_or("oversized delivery must halt")?;
        assert_eq!(error.kind(), ConsumeErrorKind::Protocol);
        observe_pull(&mut observer).await?;
        // Keep the adapter alive and do not poll it during the expiry window.
        // Retaining the old SDK stream issues another observable pull here.
        assert!(tokio::time::timeout(EXPIRY_OBSERVATION, observer.next()).await.is_err(), "halted worker issued an expiry pull");

        let control_name = resource("active_control");
        let control = durable(&stream, &control_name, CONTROL).await?;
        let mut active = JetStreamConsumer::new(control).await?;
        let mut control_observer = observer_client.subscribe(format!("$JS.API.CONSUMER.MSG.NEXT.{}.{}", stream.cached_info().config.name, control_name)).await?;
        observer_client.flush().await?;
        let reconnect_proof = async {
            observe_pull(&mut control_observer).await?;
            client.force_reconnect().await?;
            wait_connected(&mut events_rx).await?;
            // An active worker on the SAME connection must respond to the
            // reconnect. This positive witness prevents a vacuous absence test.
            observe_pull(&mut control_observer).await?;
            assert!(tokio::time::timeout(Duration::from_secs(1), observer.next()).await.is_err(), "halted worker issued a reconnect pull");
            Ok::<(), TestError>(())
        };
        tokio::select! {
            result = active.receive() => return Err(format!("empty active control unexpectedly completed: {:?}", result.err()).into()),
            result = reconnect_proof => result?,
        }
        assert_eq!(halted.receive().await.err().map(|error| error.kind()), Some(ConsumeErrorKind::Protocol));
        let info = inspector.get_info().await?;
        assert_eq!(info.num_ack_pending, 1);
        assert_eq!(info.delivered.consumer_sequence, 1);
        assert_eq!(stream.get_info().await?.state.messages, 1);
        Ok(())
    }).await
}

async fn wait_connected(events: &mut tokio::sync::mpsc::Receiver<async_nats::Event>) -> TestResult {
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
    .await?
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
    with_stream(&context, &[CLOSED], |stream| async move {
        let name = resource("closed");
        let configured = durable(&stream, &name, CLOSED).await?;
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
                result = observe_pull(&mut observer) => result?,
            }
            // Cancel a polled receive, not the adapter. The outstanding broker
            // request can still deliver; the next receive must retain ownership.
        }
        jetstream::new(client.clone()).publish(CLOSED, "bounded synthetic payload".into()).await?.await?;
        let recovered = consumer.receive().await?;
        assert_eq!(recovered.metadata().delivery_attempt(), 1);
        recovered.settle(DeliveryDisposition::Acknowledge).await?;
        let deletion = async {
            observe_pull(&mut observer).await?;
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
