//! Opt-in end-to-end handler conformance against isolated PostgreSQL.

use edgeagent_contracts::{
    Component, MessageDefinition, MessageEnvelope, MessageMetadata, MessageRegistry,
};
use edgeagent_inbox_handler::{
    HandlerError, HandlerErrorKind, HandlerFailure, HandlerPolicy, HandlingOutcome,
    QuarantineDisposition, handle_once as coordinate_once,
};
use edgeagent_inbox_postgres::{
    PostgresHandlerFuture, PostgresInboundMessageStore, PostgresInbox,
    PostgresTransactionalMessageHandler,
};
use edgeagent_messaging::{
    ConsumeError, DeliveryAttempt, DeliveryDisposition, DeliveryMessageKey, DeliveryMetadata,
    DeliverySettlement, DeliverySubject, MessageDelivery, SettlementFuture,
};
use serde_json::json;
use sqlx::{Connection, PgConnection, Postgres, Row, Transaction};
use std::env;
use std::error::Error;
use std::io;
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

fn envelope(message_id: &str) -> Result<MessageEnvelope, Box<dyn Error>> {
    Ok(COMMAND.build(
        MessageMetadata {
            id: message_id.to_owned(),
            source: Component::Gateway.source_uri().to_owned(),
            message_type: COMMAND.message_type().to_owned(),
            subject: format!("order/{message_id}"),
            time: "2026-09-28T00:00:00Z".to_owned(),
            data_schema: COMMAND.data_schema().to_owned(),
            correlation_id: format!("correlation-{message_id}"),
            causation_id: format!("request-{message_id}"),
            idempotency_key: message_id.to_owned(),
            partition_key: format!("order/{message_id}"),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
            trace_state: None,
        },
        &json!({"mode": "dry_run"}),
    )?)
}

#[derive(Clone, Copy)]
enum Behavior {
    Apply,
    FailFirstTransient,
    Permanent,
}

struct TestHandler {
    behavior: Behavior,
    calls: AtomicUsize,
}

impl TestHandler {
    const fn new(behavior: Behavior) -> Self {
        Self {
            behavior,
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl PostgresTransactionalMessageHandler for TestHandler {
    fn handle<'handler>(
        &'handler self,
        transaction: &'handler mut Transaction<'_, Postgres>,
        envelope: &'handler MessageEnvelope,
    ) -> PostgresHandlerFuture<'handler> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            sqlx::query("INSERT INTO handler_effects (message_source, message_id) VALUES ($1, $2)")
                .bind(envelope.source())
                .bind(envelope.id())
                .execute(&mut **transaction)
                .await
                .map_err(|error| {
                    HandlerFailure::with_source(
                        edgeagent_inbox_handler::HandlerFailureKind::Transient,
                        "storage_unavailable",
                        error,
                    )
                })?;
            if matches!(self.behavior, Behavior::FailFirstTransient) && call == 1 {
                return Err(HandlerFailure::transient("dependency_unavailable"));
            }
            if matches!(self.behavior, Behavior::Permanent) {
                return Err(HandlerFailure::permanent("policy_rejected"));
            }
            Ok(())
        })
    }
}

async fn handle_once(
    client: &mut PgConnection,
    registry: &MessageRegistry<'_>,
    handler: &dyn PostgresTransactionalMessageHandler,
    policy: &HandlerPolicy,
    delivery: MessageDelivery,
) -> Result<HandlingOutcome, HandlerError> {
    let mut store = PostgresInboundMessageStore::new(client, handler);
    coordinate_once(&mut store, registry, policy, delivery).await
}

#[derive(Clone)]
struct SettlementProbe {
    dispositions: Arc<Mutex<Vec<DeliveryDisposition>>>,
    fail_confirmation: bool,
}

impl DeliverySettlement for SettlementProbe {
    fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture {
        Box::pin(async move {
            self.dispositions
                .lock()
                .map_err(|_| ConsumeError::unavailable("settlement probe lock was poisoned"))?
                .push(disposition);
            if self.fail_confirmation {
                Err(ConsumeError::unavailable(
                    "settlement probe confirmation failed",
                ))
            } else {
                Ok(())
            }
        })
    }
}

fn delivery(
    payload: Vec<u8>,
    message_key: &str,
    attempt: u32,
    dispositions: Arc<Mutex<Vec<DeliveryDisposition>>>,
    fail_confirmation: bool,
) -> Result<MessageDelivery, Box<dyn Error>> {
    let metadata = DeliveryMetadata::new(
        DeliveryMessageKey::new(message_key)?,
        DeliverySubject::new(COMMAND.subject()?)?,
        DeliveryAttempt::new(attempt)?,
    );
    Ok(MessageDelivery::new(
        payload,
        metadata,
        Box::new(SettlementProbe {
            dispositions,
            fail_confirmation,
        }),
    ))
}

fn observed(
    dispositions: &Arc<Mutex<Vec<DeliveryDisposition>>>,
) -> Result<Vec<DeliveryDisposition>, io::Error> {
    dispositions
        .lock()
        .map(|values| values.clone())
        .map_err(|_| io::Error::other("settlement probe lock was poisoned"))
}

#[tokio::test]
#[ignore = "requires EDGEAGENT_POSTGRES_URL and an isolated PostgreSQL database"]
async fn coordinator_preserves_commit_retry_and_quarantine_ordering() -> Result<(), Box<dyn Error>>
{
    let postgres_url = env::var("EDGEAGENT_POSTGRES_URL")?;
    let mut client = PgConnection::connect(&postgres_url).await?;
    let schema = format!("edgeagent_handler_test_{}", process::id());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; SET search_path TO {schema};"
    )))
    .execute(&mut client)
    .await?;
    for migration in PostgresInbox::MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut client).await?;
    }
    sqlx::raw_sql(
            "CREATE TABLE handler_effects (message_source TEXT NOT NULL, message_id VARCHAR(128) NOT NULL, PRIMARY KEY (message_source, message_id))",
        )
        .execute(&mut client)
        .await?;

    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = HandlerPolicy::new(
        "execution_simulator_v1",
        3,
        Duration::from_millis(2),
        Duration::from_millis(20),
    )?;

    let applied_handler = TestHandler::new(Behavior::Apply);
    let applied = envelope("handler-applied-01")?;
    let applied_bytes = applied.to_json()?;
    let applied_settlements = Arc::new(Mutex::new(Vec::new()));
    assert!(matches!(
        handle_once(
            &mut client,
            &registry,
            &applied_handler,
            &policy,
            delivery(
                applied_bytes.clone(),
                "18:EDGEAGENT_COMMANDS:11",
                1,
                Arc::clone(&applied_settlements),
                false,
            )?,
        )
        .await?,
        HandlingOutcome::Applied { attempt: 1 }
    ));
    assert!(matches!(
        handle_once(
            &mut client,
            &registry,
            &applied_handler,
            &policy,
            delivery(
                applied_bytes,
                "18:EDGEAGENT_COMMANDS:11",
                2,
                Arc::clone(&applied_settlements),
                false,
            )?,
        )
        .await?,
        HandlingOutcome::Duplicate { attempt: 2 }
    ));
    assert_eq!(applied_handler.calls(), 1);
    assert_eq!(
        observed(&applied_settlements)?,
        vec![
            DeliveryDisposition::Acknowledge,
            DeliveryDisposition::Acknowledge
        ]
    );

    let transient_handler = TestHandler::new(Behavior::FailFirstTransient);
    let transient = envelope("handler-transient-01")?.to_json()?;
    let transient_settlements = Arc::new(Mutex::new(Vec::new()));
    assert!(matches!(
        handle_once(
            &mut client,
            &registry,
            &transient_handler,
            &policy,
            delivery(
                transient.clone(),
                "18:EDGEAGENT_COMMANDS:12",
                1,
                Arc::clone(&transient_settlements),
                false,
            )?,
        )
        .await?,
        HandlingOutcome::RetryRequested {
            attempt: 1,
            delay: _,
            failure: _,
        }
    ));
    let rolled_back = sqlx::query(
        "SELECT (SELECT count(*) FROM handler_effects WHERE message_id = $1), (SELECT count(*) FROM edgeagent_message_inbox WHERE consumer_name = $2 AND message_id = $1)",
    )
        .bind("handler-transient-01")
        .bind("execution_simulator_v1")
        .fetch_one(&mut client)
        .await?;
    assert_eq!(rolled_back.try_get::<i64, _>(0)?, 0);
    assert_eq!(rolled_back.try_get::<i64, _>(1)?, 0);
    assert!(matches!(
        handle_once(
            &mut client,
            &registry,
            &transient_handler,
            &policy,
            delivery(
                transient,
                "18:EDGEAGENT_COMMANDS:12",
                2,
                Arc::clone(&transient_settlements),
                false,
            )?,
        )
        .await?,
        HandlingOutcome::Applied { attempt: 2 }
    ));

    let poison_settlements = Arc::new(Mutex::new(Vec::new()));
    let poison_key = "18:EDGEAGENT_COMMANDS:13";
    let poison_first = handle_once(
        &mut client,
        &registry,
        &applied_handler,
        &policy,
        delivery(
            b"{not-json".to_vec(),
            poison_key,
            1,
            Arc::clone(&poison_settlements),
            true,
        )?,
    )
    .await;
    assert_eq!(
        poison_first.err().map(|error| error.kind()),
        Some(HandlerErrorKind::Settlement)
    );
    assert!(matches!(
        handle_once(
            &mut client,
            &registry,
            &applied_handler,
            &policy,
            delivery(
                b"{not-json".to_vec(),
                poison_key,
                2,
                Arc::clone(&poison_settlements),
                false,
            )?,
        )
        .await?,
        HandlingOutcome::Quarantined {
            attempt: 2,
            disposition: QuarantineDisposition::AlreadyPresent,
            failure_code: "envelope_invalid",
            failure: _,
        }
    ));

    let permanent_handler = TestHandler::new(Behavior::Permanent);
    let permanent_settlements = Arc::new(Mutex::new(Vec::new()));
    assert!(matches!(
        handle_once(
            &mut client,
            &registry,
            &permanent_handler,
            &policy,
            delivery(
                envelope("handler-permanent-01")?.to_json()?,
                "18:EDGEAGENT_COMMANDS:14",
                1,
                Arc::clone(&permanent_settlements),
                false,
            )?,
        )
        .await?,
        HandlingOutcome::Quarantined {
            attempt: 1,
            disposition: QuarantineDisposition::Inserted,
            failure_code: "policy_rejected",
            failure: _,
        }
    ));

    let acknowledgement_handler = TestHandler::new(Behavior::Apply);
    let acknowledgement = envelope("handler-ack-loss-01")?.to_json()?;
    let acknowledgement_settlements = Arc::new(Mutex::new(Vec::new()));
    let acknowledgement_loss = handle_once(
        &mut client,
        &registry,
        &acknowledgement_handler,
        &policy,
        delivery(
            acknowledgement.clone(),
            "18:EDGEAGENT_COMMANDS:15",
            1,
            Arc::clone(&acknowledgement_settlements),
            true,
        )?,
    )
    .await;
    assert_eq!(
        acknowledgement_loss.err().map(|error| error.kind()),
        Some(HandlerErrorKind::Settlement)
    );
    assert!(matches!(
        handle_once(
            &mut client,
            &registry,
            &acknowledgement_handler,
            &policy,
            delivery(
                acknowledgement,
                "18:EDGEAGENT_COMMANDS:15",
                2,
                Arc::clone(&acknowledgement_settlements),
                false,
            )?,
        )
        .await?,
        HandlingOutcome::Duplicate { attempt: 2 }
    ));
    assert_eq!(acknowledgement_handler.calls(), 1);

    let counts = sqlx::query(
        "SELECT (SELECT count(*) FROM handler_effects), (SELECT count(*) FROM edgeagent_message_quarantine)",
    )
        .fetch_one(&mut client)
        .await?;
    assert_eq!(counts.try_get::<i64, _>(0)?, 3);
    assert_eq!(counts.try_get::<i64, _>(1)?, 2);

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "SET search_path TO public; DROP SCHEMA {schema} CASCADE;"
    )))
    .execute(&mut client)
    .await?;
    Ok(())
}
