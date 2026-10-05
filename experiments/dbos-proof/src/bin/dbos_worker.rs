//! Child process for the native DBOS SDK crash/restart proof.

use std::error::Error;

use dbos::{Config, DBOS, StartOptions};
use serde::{Deserialize, Serialize};
use tokio_postgres::NoTls;

#[derive(Debug, Deserialize, Serialize, thiserror::Error)]
enum ProofError {
    #[error("synthetic step database error: {0}")]
    Database(String),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url = std::env::var("DBOS_PROOF_DATABASE_URL")?;
    let workflow_id = std::env::var("DBOS_PROOF_WORKFLOW_ID")?;
    let phase = std::env::var("DBOS_PROOF_PHASE")?;
    let mut config = Config::new("edgeagent-m08-proof", database_url.clone());
    config.app_version = Some("m08-proof-v1".to_owned());
    let dbos = DBOS::new(config);

    // The workflow registration and version are identical in both processes.
    let workflow: dbos::WorkflowRef<(), String, ProofError> =
        dbos.register_workflow("m08_native_crash_recovery", move |_: ()| {
            let database_url = database_url.clone();
            async move {
                let step_a = dbos::step("step_a", || {
                    let database_url = database_url.clone();
                    async move {
                        bump(&database_url, "a").await?;
                        Ok::<_, dbos::Error<ProofError>>("a-complete".to_owned())
                    }
                })
                .await?;
                let step_b = dbos::step("step_b", || {
                    let database_url = database_url.clone();
                    async move {
                        let attempts = bump(&database_url, "b").await?;
                        if attempts == 1 {
                            // Abrupt process termination, bypassing graceful DBOS shutdown.
                            std::process::abort();
                        }
                        Ok::<_, dbos::Error<ProofError>>("b-complete".to_owned())
                    }
                })
                .await?;
                Ok::<String, dbos::Error<ProofError>>(format!("{step_a}:{step_b}"))
            }
        })?;

    dbos.launch().await?;
    match phase.as_str() {
        "first" => {
            let handle = workflow
                .start_with(
                    (),
                    StartOptions {
                        workflow_id: Some(&workflow_id),
                        ..Default::default()
                    },
                )
                .await?;
            let result = handle.result().await?;
            return Err(format!("worker did not crash; unexpected result {result}").into());
        }
        "resume" => {
            let handle = dbos.retrieve_workflow::<String, ProofError>(&workflow_id)?;
            let result = handle.result().await?;
            if result != "a-complete:b-complete" {
                return Err(format!("unexpected recovered result: {result}").into());
            }
            dbos.shutdown().await;
        }
        other => return Err(format!("unknown phase: {other}").into()),
    }
    Ok(())
}

async fn bump(database_url: &str, step: &str) -> Result<i32, ProofError> {
    let (client, connection) = tokio_postgres::connect(database_url, NoTls)
        .await
        .map_err(|error| ProofError::Database(error.to_string()))?;
    let connection_task = tokio::spawn(connection);
    let count: i32 = client
        .query_one(
            "INSERT INTO app.m08_step_counts (step, runs) VALUES ($1, 1)
             ON CONFLICT (step) DO UPDATE SET runs = app.m08_step_counts.runs + 1
             RETURNING runs",
            &[&step],
        )
        .await
        .map_err(|error| ProofError::Database(error.to_string()))?
        .get(0);
    connection_task.abort();
    Ok(count)
}
