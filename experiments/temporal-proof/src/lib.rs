//! Synthetic M08 point-in-time evaluation workflow for the Temporal decision gate.
//!
//! No model provider is called. Activity B deliberately aborts its worker on its first
//! attempt. The workflow input identifies a frozen manifest and test evidence directory.

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::Duration,
};
use temporalio_macros::{activities, workflow, workflow_methods};
use temporalio_sdk::{
    ActivityOptions, WorkflowContext, WorkflowResult,
    activities::{ActivityContext, ActivityError},
};

/// A frozen synthetic M08 manifest and a private location for test evidence.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct M08Input {
    /// Immutable identity of the evaluated snapshot.
    pub manifest_id: String,
    /// Directory created by the test, never a production artifact location.
    pub evidence_dir: String,
}

/// The workflow coordinates two durable activities; it never performs I/O itself.
#[workflow]
#[derive(Default)]
pub struct M08Workflow;

#[workflow_methods]
impl M08Workflow {
    /// A is committed before B is scheduled; replay must not execute A again.
    #[run]
    pub async fn run(ctx: &mut WorkflowContext<Self>, input: M08Input) -> WorkflowResult<String> {
        let manifest_id = input.manifest_id.clone();
        let prepared = ctx
            .execute_activity(
                M08Activities::step_a,
                input.clone(),
                ActivityOptions::start_to_close_timeout(Duration::from_secs(10)),
            )
            .await?;
        let evaluated = ctx
            .execute_activity(
                M08Activities::step_b,
                input,
                ActivityOptions::start_to_close_timeout(Duration::from_secs(3)),
            )
            .await?;
        Ok(format!("{manifest_id}:{prepared}:{evaluated}"))
    }
}

/// Test-only activities; model work is represented by a deterministic marker.
pub struct M08Activities;

#[activities]
impl M08Activities {
    /// Simulated local preparation, observed with an append-only execution counter.
    #[activity]
    pub async fn step_a(_ctx: ActivityContext, input: M08Input) -> Result<String, ActivityError> {
        let path = Path::new(&input.evidence_dir).join("step-a-invocations");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("test evidence directory must be writable");
        writeln!(file, "{}", input.manifest_id).expect("test counter write must succeed");
        file.sync_all()
            .expect("test counter must be durable before activity completion");
        Ok("prepared".to_owned())
    }

    /// Simulated provider interaction. The first invocation crashes the whole worker.
    #[activity]
    pub async fn step_b(_ctx: ActivityContext, input: M08Input) -> Result<String, ActivityError> {
        let first_attempt_marker = Path::new(&input.evidence_dir).join("step-b-first-attempt");
        if !first_attempt_marker.exists() {
            fs::write(&first_attempt_marker, b"entered before worker crash")
                .expect("test crash marker write must succeed");
            // This is a process crash, not a returned activity failure. The server must
            // time out this attempt and schedule a fresh one on the restarted worker.
            std::process::abort();
        }
        fs::write(
            Path::new(&input.evidence_dir).join("step-b-completed"),
            b"completed after restart",
        )
        .expect("test completion marker write must succeed");
        Ok("evaluated".to_owned())
    }
}
