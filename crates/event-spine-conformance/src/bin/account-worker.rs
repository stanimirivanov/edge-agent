//! Private finite qualification worker. Never deployed; faults wrap the real
//! handler, not production adapters. Parent owns all fixture schemas/processes.
use edgeagent_contracts::{MessageEnvelope, simulation::registry};
use edgeagent_event_spine_conformance::accounts::{Error, Scenario};
use edgeagent_inbox_postgres::{
    PostgresHandlerFuture, PostgresInboundMessageStore, PostgresTransactionalMessageHandler,
};
use edgeagent_messaging::{FailureCode, HandlerFailure, InboundMessageStore};
use edgeagent_simulation_postgres::provision;
use sqlx::{
    ConnectOptions, Connection, PgConnection, Postgres, Transaction, postgres::PgConnectOptions,
};
use std::{io::Write, process::ExitCode, time::Duration};
use tokio::io::AsyncReadExt;

struct Wrapper<'context> {
    real: &'context dyn PostgresTransactionalMessageHandler,
    mode: String,
}
impl PostgresTransactionalMessageHandler for Wrapper<'_> {
    fn handle<'a>(
        &'a self,
        transaction: &'a mut Transaction<'_, Postgres>,
        envelope: &'a MessageEnvelope,
    ) -> PostgresHandlerFuture<'a> {
        Box::pin(async move {
            self.real.handle(transaction, envelope).await?;
            if self.mode == "rollback" {
                return Err(HandlerFailure::permanent(FailureCode::from_static(
                    "p08_injected_failure",
                )));
            }
            if self.mode == "before" {
                barrier("READY").await.map_err(|_| {
                    HandlerFailure::transient(FailureCode::from_static("p08_parent_closed"))
                })?;
            }
            Ok(())
        })
    }
}
async fn barrier(stage: &str) -> Result<(), Error> {
    println!("{stage}");
    std::io::stdout().flush()?;
    let mut byte = [0];
    tokio::io::stdin().read_exact(&mut byte).await?;
    Ok(())
}
async fn run() -> Result<(), Error> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [case, slot, mode, principal] = args.as_slice() else {
        return Err("invalid qualification arguments".into());
    };
    if !matches!(
        mode.as_str(),
        "initialize" | "apply" | "before" | "after" | "rollback"
    ) {
        return Err("invalid qualification stage".into());
    }
    let schema = std::env::var("P08_SCHEMA")?;
    if !schema.starts_with("p08_")
        || schema.len() > 63
        || !schema
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err("invalid owned schema".into());
    }
    let options: PgConnectOptions = std::env::var("EDGEAGENT_POSTGRES_URL")?.parse()?;
    let mut options = options.options([
        ("search_path", schema.as_str()),
        ("lock_timeout", "5s"),
        ("statement_timeout", "15s"),
        ("idle_in_transaction_session_timeout", "20s"),
    ]);
    if let Ok(role) = std::env::var("P08_ROLE") {
        options = options.username(&role).password("p08-public-fixture-only");
    }
    options = options
        .application_name(&std::env::var("P08_APPLICATION")?)
        .disable_statement_logging();
    let mut connection = PgConnection::connect_with(&options)
        .await
        .inspect_err(|error| {
            let code = error.as_database_error().and_then(|db| db.code());
            eprintln!(
                "fixture connection rejected; sqlstate={}",
                code.as_deref().unwrap_or("unavailable")
            );
        })?;
    let scenario = Scenario::new(case).inspect_err(|_| eprintln!("fixture scenario rejected"))?;
    let context = scenario
        .context(principal.parse()?)
        .inspect_err(|_| eprintln!("fixture admission context rejected"))?;
    let execution = if scenario.has_execution() {
        Some(
            scenario
                .execution(&context)
                .inspect_err(|_| eprintln!("fixture execution context rejected"))?,
        )
    } else {
        None
    };
    let settlement = if scenario.has_settlement() {
        Some(scenario.settlement(execution.as_ref().ok_or("execution absent")?)?)
    } else {
        None
    };
    if mode == "initialize" {
        provision(
            &mut connection,
            &context,
            scenario.funding(),
            &scenario.valuations()?,
            &scenario.inventory()?,
        )
        .await
        .inspect_err(|_| eprintln!("fixture account provisioning rejected"))?;
        if let Some(execution) = &execution {
            edgeagent_simulation_postgres::provision_execution(
                &mut connection,
                execution,
                &scenario.costs()?,
            )
            .await
            .inspect_err(|_| eprintln!("fixture execution provisioning rejected"))?;
        }
        if let Some(settlement) = &settlement {
            edgeagent_simulation_postgres::provision_settlement(&mut connection, settlement)
                .await?;
        }
    } else {
        let index: usize = slot.parse()?;
        let id = std::env::var("P08_MESSAGE_ID")?;
        let settling = scenario.has_settlement() && index >= 5;
        let executing = scenario.has_execution() && index >= 2 && !settling;
        let mut envelope = if settling {
            scenario.settlement_envelope(
                settlement.as_ref().ok_or("settlement absent")?,
                index as u64 + 1,
                &id,
                std::env::var("P08_CONFLICT").as_deref() == Ok("1"),
            )?
        } else if executing {
            scenario.execution_envelope(
                execution.as_ref().ok_or("execution absent")?,
                index as u64 + 1,
                &id,
                std::env::var("P08_CONFLICT").as_deref() == Ok("1"),
            )?
        } else {
            scenario.envelope(index, &id)?
        };
        if !executing && !settling && std::env::var("P08_CONFLICT").as_deref() == Ok("1") {
            let original = scenario.request(index)?;
            let identity = original.identity();
            let request = edgeagent_contracts::simulation::SubmitDryRunOrder::new(
                "dry_run",
                identity.0,
                identity.1,
                identity.2,
                99,
                original.position().1,
            )?;
            envelope = Scenario::wrap(&request, &id)?;
        }
        let wrapper = Wrapper {
            real: if settling {
                settlement.as_ref().ok_or("settlement absent")?
            } else if executing {
                execution.as_ref().ok_or("execution absent")?
            } else {
                &context
            },
            mode: mode.clone(),
        };
        let registry = if settling {
            edgeagent_contracts::settlement::registry()?
        } else if executing {
            edgeagent_contracts::execution::registry()?
        } else {
            registry()?
        };
        if let Err(error) = PostgresInboundMessageStore::new(&mut connection, &wrapper)
            .process(
                if settling {
                    "p10-settlement"
                } else if executing {
                    "p09-execution"
                } else {
                    "p08-admission"
                },
                &registry,
                &envelope,
            )
            .await
        {
            if let edgeagent_messaging::InboundProcessingError::Handler(failure) = &error {
                println!("REJECT {}", failure.code().as_str());
            }
            return Err(error.into());
        }
        if mode == "after" {
            barrier("COMMITTED").await?;
        }
    }
    connection.close().await?;
    println!("DONE");
    Ok(())
}
fn main() -> ExitCode {
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    else {
        return ExitCode::FAILURE;
    };
    let result =
        runtime.block_on(async { tokio::time::timeout(Duration::from_secs(30), run()).await });
    runtime.shutdown_timeout(Duration::from_secs(1));
    match result {
        Ok(Ok(())) => ExitCode::SUCCESS,
        _ => {
            eprintln!("account qualification worker failed; durable result may be unresolved");
            ExitCode::FAILURE
        }
    }
}
