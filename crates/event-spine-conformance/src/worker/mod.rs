mod ordering;
mod persistence;
mod replay;
mod telemetry;

use std::{error::Error, io::Write, time::Duration};

use async_nats::jetstream;
use edgeagent_contracts::{
    Component, MAX_PORTABLE_MESSAGE_BYTES, MessageEnvelope, MessageMetadata,
};
use edgeagent_event_spine_conformance::{AcceptanceMessage, ApplyProbe, definitions, registry};
use edgeagent_inbox_handler::{HandlerPolicy, HandlingOutcome, MessageFailure, handle_once};
use edgeagent_inbox_postgres::PostgresInboundMessageStore;
use edgeagent_messaging::{LeaseDuration, MessageConsumer};
use edgeagent_messaging_nats::topology::{Binding, TopologyLimits, TopologyPlan};
use edgeagent_messaging_nats::{JetStreamConsumer, JetStreamPublisher};
use edgeagent_outbox_postgres::PostgresOutboxRelay;
use edgeagent_outbox_relay::{MaxAttempts, RelayOutcome, RelayPolicy, RetryBackoff, relay_once};
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use tokio::io::AsyncReadExt;

type WorkerError = Box<dyn Error + Send + Sync>;
const IO_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
enum Role {
    Producer,
    Relay,
    Consumer,
    ReplayAuthorize,
    ReplayDispatch,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Self::Producer => "producer",
            Self::Relay => "relay",
            Self::Consumer => "consumer",
            Self::ReplayAuthorize => "replay-authorize",
            Self::ReplayDispatch => "replay-dispatch",
        }
    }

    fn component(self) -> Result<Component, WorkerError> {
        if matches!(self, Self::Producer) {
            return Ok(Component::Gateway);
        }
        match std::env::var("EDGEAGENT_PROOF_COMPONENT")?.as_str() {
            "gateway" => Ok(Component::Gateway),
            "research" => Ok(Component::Research),
            "auditor" => Ok(Component::AuditProjector),
            _ => Err("invalid runtime component".into()),
        }
    }

    fn parse(value: &str) -> Result<Self, WorkerError> {
        match value {
            "producer" => Ok(Self::Producer),
            "relay" => Ok(Self::Relay),
            "consumer" => Ok(Self::Consumer),
            "replay-authorize" => Ok(Self::ReplayAuthorize),
            "replay-dispatch" => Ok(Self::ReplayDispatch),
            _ => Err("invalid finite worker role".into()),
        }
    }
}

// Identifiers enter SET search_path and resource names, never arbitrary SQL.
fn identifier(value: &str) -> bool {
    (1..=32).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && value.starts_with("m02_")
}

pub(super) async fn run() -> Result<(), WorkerError> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let [role] = arguments.as_slice() else {
        return Err("expected one role".into());
    };
    let role = Role::parse(role)?;
    let schema = std::env::var("EDGEAGENT_PROOF_SCHEMA")?;
    if !identifier(&schema) {
        return Err("invalid owned schema".into());
    }
    let namespace = std::env::var("EDGEAGENT_PROOF_NAMESPACE")?;
    let plan = TopologyPlan::new(
        registry()?,
        &namespace,
        &[Binding::new(
            Component::Gateway,
            definitions()[0].message_type(),
        )],
        &[Binding::new(
            Component::AuditProjector,
            definitions()[1].message_type(),
        )],
        TopologyLimits::default(),
    )?;
    let options: PgConnectOptions = std::env::var("EDGEAGENT_POSTGRES_URL")?.parse()?;
    let options = options
        .options([
            ("search_path", schema.as_str()),
            ("statement_timeout", "5000"),
            ("lock_timeout", "2000"),
        ])
        .application_name(&format!("m02_worker_{}", std::process::id()));
    let mut database = PgConnection::connect_with(&options).await?;
    let component = role.component()?;
    let context = if matches!(role, Role::Producer | Role::ReplayAuthorize) {
        None
    } else {
        if matches!(role, Role::Consumer) && component != Component::Research {
            return Err("command handler must be research".into());
        }
        let permissions = plan
            .runtime_permissions()
            .iter()
            .find(|permission| permission.component() == Some(component))
            .ok_or("missing runtime permission")?;
        let replay_scope = if matches!(role, Role::ReplayDispatch) {
            Some(
                edgeagent_event_spine_conformance::replay::ReplayPublisherScope::new(
                    &plan, component,
                )?,
            )
        } else {
            None
        };
        let (user, inbox) = replay_scope
            .as_ref()
            .map_or((component.as_str(), permissions.inbox_prefix()), |scope| {
                (scope.user(), scope.inbox_prefix())
            });
        let client = async_nats::ConnectOptions::new()
            .user_and_password(user.into(), "fixture-only".into())
            .custom_inbox_prefix(inbox)
            .request_timeout(Some(IO_DEADLINE))
            .connect(std::env::var("EDGEAGENT_NATS_URL")?)
            .await?;
        Some(
            jetstream::ContextBuilder::new()
                .timeout(IO_DEADLINE)
                .build(client),
        )
    };
    // The parent observes readiness before permitting one finite operation.
    println!("ready");
    std::io::stdout().flush()?;
    let mut action = [0];
    let mut input = tokio::io::stdin();
    input.read_exact(&mut action).await?;
    let hold_delivery = action[0] == b'h' && matches!(role, Role::Consumer);
    let hold_receipt = action[0] == b'p' && matches!(role, Role::ReplayDispatch);
    let rollback = match action[0] {
        b'c' => false,
        b'r' => true,
        b'h' if hold_delivery => false,
        b'p' if hold_receipt => false,
        _ => return Err("invalid action".into()),
    };
    match role {
        Role::Producer => {
            // EOF keeps the original default fixture convenient. Explicit bytes
            // use the same bounded contract decoder as every other producer.
            let mut bytes = Vec::new();
            input
                .take(u64::try_from(MAX_PORTABLE_MESSAGE_BYTES)? + 1)
                .read_to_end(&mut bytes)
                .await?;
            let envelope = if bytes.is_empty() {
                ApplyProbe::new("probe-01", 0, 42)?.envelope(metadata(false))?
            } else {
                let envelope = MessageEnvelope::from_json(&bytes)?;
                if !matches!(
                    AcceptanceMessage::from_envelope(&envelope)?,
                    AcceptanceMessage::Command(_)
                ) {
                    return Err("producer requires a command".into());
                }
                envelope
            };
            let started = std::time::Instant::now();
            let result = persistence::accept(&mut database, &envelope, rollback).await;
            edgeagent_telemetry::record_event_spine_operation(
                &edgeagent_telemetry::EventSpineContext::from_envelope(&envelope),
                edgeagent_telemetry::EventSpineStage::Persistence,
                if result.is_ok() && !rollback {
                    edgeagent_telemetry::EventSpineOutcome::Succeeded
                } else {
                    edgeagent_telemetry::EventSpineOutcome::Failed
                },
                1,
                started.elapsed(),
            );
            result?;
            println!("{}", if rollback { "rolled_back" } else { "accepted" });
        }
        Role::Relay => {
            if rollback {
                return Err("relay cannot simulate rollback".into());
            }
            let publisher = JetStreamPublisher::from_context(context.ok_or("broker absent")?);
            let policy = RelayPolicy::new(
                "m02_relay",
                LeaseDuration::new(Duration::from_secs(10))?,
                MaxAttempts::new(3)?,
                RetryBackoff::new(Duration::from_millis(10), Duration::from_secs(1))?,
            )?;
            let outcome = relay_once(
                &mut PostgresOutboxRelay::new(&mut database),
                &registry()?,
                &publisher,
                &policy,
            )
            .await?;
            match outcome {
                RelayOutcome::Published { .. } => println!("published"),
                RelayOutcome::Idle => println!("idle"),
                _ => return Err("publication not completed".into()),
            }
        }
        Role::Consumer => {
            let context = context.ok_or("broker absent")?;
            let stream = plan
                .streams()
                .iter()
                .find(|stream| stream.definition() == definitions()[0])
                .ok_or("command stream absent")?;
            let consumer = stream.consumers().first().ok_or("command durable absent")?;
            let name = consumer
                .config()
                .durable_name
                .as_deref()
                .ok_or("durable absent")?;
            let durable = context
                .get_stream(&stream.config().name)
                .await?
                .get_consumer(name)
                .await?;
            let mut consumer = JetStreamConsumer::new(durable).await?;
            let delivery = tokio::time::timeout(IO_DEADLINE, consumer.receive()).await??;
            if hold_delivery {
                // Fixture-only intake barrier: no transaction is open. This
                // exposes broker admission without production adapter hooks.
                println!("delivered");
                std::io::stdout().flush()?;
                input.read_exact(&mut action).await?;
                if action[0] != b'c' {
                    return Err("invalid delivery release".into());
                }
            }
            let handler = persistence::ProbeHandler { rollback };
            let mut store = PostgresInboundMessageStore::new(&mut database, &handler);
            let policy =
                HandlerPolicy::new(name, 5, Duration::from_millis(10), Duration::from_secs(1))?;
            let outcome = handle_once(&mut store, &registry()?, &policy, delivery).await?;
            match (rollback, outcome) {
                (false, HandlingOutcome::Applied { .. }) => println!("applied"),
                (false, HandlingOutcome::Duplicate { .. }) => println!("duplicate"),
                (
                    true,
                    HandlingOutcome::RetryRequested {
                        failure: MessageFailure::Handler(failure),
                        ..
                    },
                ) if failure.code().as_str() == "fixture_rollback" => println!("rolled_back"),
                (
                    false,
                    HandlingOutcome::RetryRequested {
                        failure: MessageFailure::Handler(failure),
                        ..
                    },
                ) if failure.code().as_str() == "missing_predecessor" => println!("deferred"),
                (false, HandlingOutcome::RetryRequested { .. }) => println!("retry"),
                (false, HandlingOutcome::Quarantined { failure_code, .. }) => {
                    println!("quarantined:{failure_code}")
                }
                _ => return Err("unexpected handling outcome".into()),
            }
        }
        Role::ReplayAuthorize => {
            let mut bytes = Vec::new();
            input.take(4097).read_to_end(&mut bytes).await?;
            if bytes.len() > 4096 {
                return Err("replay request exceeds bound".into());
            }
            println!(
                "{}",
                replay::authorize(&mut database, &schema, &plan, component, &bytes, rollback)
                    .await?
            );
        }
        Role::ReplayDispatch => {
            if rollback {
                return Err("dispatch cannot simulate authorization rollback".into());
            }
            println!(
                "{}",
                replay::dispatch(
                    &mut database,
                    &schema,
                    &plan,
                    component,
                    context.ok_or("broker absent")?,
                    hold_receipt,
                    &mut input
                )
                .await?
            );
        }
    }
    database.close().await?;
    Ok(())
}

pub(super) fn start_telemetry() -> Result<Option<telemetry::Telemetry>, WorkerError> {
    if std::env::var_os(telemetry::ENDPOINT).is_none() {
        return Ok(None);
    }
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let [role] = arguments.as_slice() else {
        return Err("expected one role".into());
    };
    let role = Role::parse(role)?;
    telemetry::Telemetry::start(role.as_str(), role.component()?.as_str())
}

fn metadata(fact: bool) -> MessageMetadata {
    let definition = if fact {
        definitions()[1]
    } else {
        definitions()[0]
    };
    MessageMetadata {
        id: if fact {
            "probe-applied-01"
        } else {
            "apply-probe-01"
        }
        .into(),
        source: if fact {
            Component::Research
        } else {
            Component::Gateway
        }
        .source_uri()
        .into(),
        message_type: definition.message_type().into(),
        subject: "probe/probe-01".into(),
        time: "2026-10-17T00:00:00Z".into(),
        data_schema: definition.data_schema().into(),
        correlation_id: "m02-process-01".into(),
        causation_id: if fact { "apply-probe-01" } else { "request-01" }.into(),
        idempotency_key: if fact {
            "probe-applied-01"
        } else {
            "apply-probe-01"
        }
        .into(),
        partition_key: "probe/probe-01".into(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
        trace_state: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_and_role_input_fail_closed() {
        for value in ["", "public", "m02_x;drop", "m02_../x", "m02_UPPER", "m02_é"] {
            assert!(!identifier(value));
        }
        assert!(identifier("m02_producer_123"));
        assert!(Role::parse("producer").is_ok());
        assert!(Role::parse("provisioner").is_err());
    }
}
