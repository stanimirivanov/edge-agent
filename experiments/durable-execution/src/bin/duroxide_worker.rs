//! Disposable worker used by the M08 crash-recovery proof.

use std::error::Error;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use duroxide::providers::sqlite::SqliteProvider;
use duroxide::runtime::{self, RuntimeOptions, registry::ActivityRegistry};
use duroxide::{
    ActivityContext, Client, OrchestrationContext, OrchestrationRegistry, OrchestrationStatus,
};

const INSTANCE_ID: &str = "m08-synthetic";

fn record_attempt(path: &Path, marker: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{marker}")?;
    file.sync_all()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let database_url = args.next().ok_or("missing SQLite URL")?;
    let evidence_dir = PathBuf::from(args.next().ok_or("missing evidence directory")?);
    let phase = args.next().ok_or("missing phase")?;
    if args.next().is_some() || (phase != "crash" && phase != "resume") {
        return Err("expected <sqlite-url> <evidence-dir> <crash|resume>".into());
    }

    let step_a_path = Arc::new(evidence_dir.join("step_a_attempts"));
    let step_b_path = Arc::new(evidence_dir.join("step_b_attempts"));
    let crash_during_b = phase == "crash";

    let activities = ActivityRegistry::builder()
        .register("StepA", move |_ctx: ActivityContext, input: String| {
            let path = Arc::clone(&step_a_path);
            async move {
                record_attempt(&path, "A").map_err(|error| error.to_string())?;
                Ok(input)
            }
        })
        .register("StepB", move |_ctx: ActivityContext, input: String| {
            let path = Arc::clone(&step_b_path);
            async move {
                record_attempt(&path, "B").map_err(|error| error.to_string())?;
                if crash_during_b {
                    // The test kills this worker only after this signal. Step A has
                    // completed and been checkpointed, while B is still in flight.
                    println!("STEP_B_STARTED");
                    io::stdout().flush().map_err(|error| error.to_string())?;
                    std::future::pending::<()>().await;
                }
                Ok(format!("model-result:{input}"))
            }
        })
        .build();

    let version_one = OrchestrationRegistry::builder().register(
        "M08",
        |ctx: OrchestrationContext, input: String| async move {
            let a = ctx.schedule_activity("StepA", input).await?;
            let b = ctx.schedule_activity("StepB", a).await?;
            Ok::<_, String>(b)
        },
    );
    let orchestrations = if phase == "resume" {
        // Deploying a new default version must not rewrite the command history
        // of the already-running v1 instance after the worker crashes.
        version_one.register_versioned("M08", "2.0.0", |_ctx, _input| async move {
            Ok::<_, String>("unexpected-v2".to_owned())
        })
    } else {
        version_one
    }
    .build();

    let store = Arc::new(SqliteProvider::new(&database_url, None).await?);
    let options = RuntimeOptions {
        worker_lock_timeout: Duration::from_secs(2),
        worker_lock_renewal_buffer: Duration::from_secs(1),
        orchestrator_lock_timeout: Duration::from_secs(2),
        orchestrator_lock_renewal_buffer: Duration::from_secs(1),
        dispatcher_min_poll_interval: Duration::from_millis(10),
        activity_cancellation_grace_period: Duration::from_secs(1),
        ..RuntimeOptions::default()
    };
    let runtime =
        runtime::Runtime::start_with_options(store.clone(), activities, orchestrations, options)
            .await;
    let client = Client::new(store);

    if phase == "resume" {
        let status = client
            .wait_for_orchestration(INSTANCE_ID, Duration::from_secs(25))
            .await?;
        match status {
            OrchestrationStatus::Completed { output, .. } if output == "model-result:synthetic" => {
                runtime.shutdown(None).await;
                return Ok(());
            }
            other => return Err(format!("unexpected M08 status: {other:?}").into()),
        }
    }

    // The first phase only returns if the test fails to kill the process.
    std::future::pending::<()>().await;
    Ok(())
}
