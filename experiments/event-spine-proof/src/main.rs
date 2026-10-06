//! Synthetic M08 worker using the current event-spine components as deployed.
//! Step B deliberately lives after the inbound transaction and acknowledgement:
//! a model call cannot safely run inside the PostgreSQL inbox transaction.

use edgeagent_contracts::{Component, MessageDefinition, MessageRegistry};
use edgeagent_inbox_handler::{HandlerFailure, HandlerPolicy, HandlingOutcome, handle_once};
use edgeagent_inbox_postgres::{
    PostgresHandlerFuture, PostgresInboundMessageStore, PostgresTransactionalMessageHandler,
};
use edgeagent_messaging::MessageConsumer;
use edgeagent_messaging_nats::JetStreamConsumer;
use sqlx::{Connection, PgConnection, Postgres, Transaction};
use std::{env, error::Error, io, time::Duration};

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.execution.submit-dry-run-order.v1",
    "urn:edgeagent:schema:submit-dry-run-order:v1",
    Component::ExecutionSimulator,
    "order",
);

struct StepA;

impl PostgresTransactionalMessageHandler for StepA {
    fn handle<'handler>(
        &'handler self,
        transaction: &'handler mut Transaction<'_, Postgres>,
        _envelope: &'handler edgeagent_contracts::MessageEnvelope,
    ) -> PostgresHandlerFuture<'handler> {
        Box::pin(async move {
            sqlx::query("INSERT INTO m08_step_a (id) VALUES (1)")
                .execute(&mut **transaction)
                .await
                .map_err(|error| {
                    HandlerFailure::with_source(
                        edgeagent_inbox_handler::HandlerFailureKind::Transient,
                        "storage_unavailable",
                        error,
                    )
                })?;
            Ok(())
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = env::args().skip(1);
    let postgres_url = args
        .next()
        .ok_or_else(|| io::Error::other("missing PostgreSQL URL"))?;
    let nats_url = args
        .next()
        .ok_or_else(|| io::Error::other("missing NATS URL"))?;
    let hang_at_b = args.next().as_deref() == Some("hang-at-b");

    let mut database = PgConnection::connect(&postgres_url).await?;
    let nats = async_nats::connect(&nats_url).await?;
    let stream = async_nats::jetstream::new(nats)
        .get_stream("M08_SPINE")
        .await?;
    let consumer = stream.get_consumer("m08_worker").await?;
    let mut delivery_source = JetStreamConsumer::new(consumer).await?;

    let delivery =
        match tokio::time::timeout(Duration::from_secs(3), delivery_source.receive()).await {
            Ok(result) => result?,
            Err(_) => {
                println!("NO_PENDING_DELIVERY");
                return Ok(());
            }
        };
    let definitions = [COMMAND];
    let registry = MessageRegistry::new(&definitions)?;
    let policy = HandlerPolicy::new(
        "m08_worker",
        5,
        Duration::from_millis(10),
        Duration::from_secs(1),
    )?;
    let outcome = {
        let mut store = PostgresInboundMessageStore::new(&mut database, &StepA);
        handle_once(&mut store, &registry, &policy, delivery).await?
    };
    if !matches!(outcome, HandlingOutcome::Applied { .. }) {
        return Err(io::Error::other("Step A was not applied").into());
    }

    // The inbox commit and broker acknowledgement have completed. The event
    // spine has no durable continuation record for this next logical step.
    sqlx::query("INSERT INTO m08_step_b_started (id) VALUES (1)")
        .execute(&mut database)
        .await?;
    if hang_at_b {
        std::future::pending::<()>().await;
    }
    sqlx::query("INSERT INTO m08_step_b_completed (id) VALUES (1)")
        .execute(&mut database)
        .await?;
    Ok(())
}
