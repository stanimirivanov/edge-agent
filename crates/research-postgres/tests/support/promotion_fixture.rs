//! Invented, exact-profile promotion evidence for isolated Research integration tests.
use edgeagent_contracts::{
    research::ResearchRecord,
    research_experiment::{Action, Command, ExperimentResult, Terminal, Trial},
    research_promotion::Promotion,
    strategy_governance::Profile,
};
use edgeagent_research_postgres::PostgresResearchStore;

type FixtureResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn promotion(profile: &Profile) -> FixtureResult<(Promotion, [Command; 3])> {
    let fixture = include_bytes!("../../../research-application/tests/fixtures/artifact-v1.json");
    let ResearchRecord::Artifact(mut artifact) = ResearchRecord::decode(fixture)? else {
        return Err("expected fixture artifact".into());
    };
    artifact.request.scope.clone_from(&profile.scope);
    artifact
        .request
        .implementation
        .clone_from(&profile.implementation);
    artifact
        .request
        .configuration
        .clone_from(&profile.configuration);
    artifact.request.lookback_sessions = profile.lookback_sessions;
    let trial = artifact.request.request_id.clone();
    let family = format!(
        "promotion-{}",
        profile.digest()?.get(..32).ok_or("profile digest")?
    );
    let declared = Trial {
        request: artifact.request.clone(),
        evidence: artifact.evidence.clone(),
    };
    let record = ResearchRecord::Artifact(artifact);
    let mut wrapper =
        br#"{"kind":"research.experiment.v1","publication_eligible":false,"result":"#.to_vec();
    wrapper.extend_from_slice(
        record
            .canonical_bytes()?
            .strip_suffix(b"\n")
            .ok_or("fixture newline")?,
    );
    wrapper.extend_from_slice(b"}\n");
    let command = |action| Command::V1 {
        scope: profile.scope.clone(),
        family: family.clone(),
        action,
    };
    let commands = [
        command(Action::Register {
            protocol: "ab".repeat(32),
            trials: vec![declared],
        }),
        command(Action::Finish {
            trial: trial.clone(),
            outcome: Terminal::Result {
                result: ExperimentResult::decode(&wrapper)?,
            },
        }),
        command(Action::Select {
            trial: trial.clone(),
        }),
    ];
    Ok((
        Promotion {
            profile: profile.clone(),
            family,
            trial,
            result: record.digest()?,
            report: "ef".repeat(32),
            holdout_start_ms: 101,
            holdout_end_ms: 200,
        },
        commands,
    ))
}

pub async fn retain(store: &PostgresResearchStore, profile: &Profile) -> FixtureResult<Promotion> {
    let (evidence, commands) = promotion(profile)?;
    for command in &commands {
        store.experiment_ledger(command).await?;
    }
    Ok(evidence)
}
