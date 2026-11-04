use edgeagent_contracts::{
    research::ResearchRecord,
    research_experiment::{Action, Command, ExperimentResult, Failure, Terminal, Trial},
};
use edgeagent_research_application::experiment_ledger::Ledger;

#[test]
fn selection_requires_every_registered_attempt_and_pins_its_successful_result()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = include_bytes!("fixtures/artifact-v1.json");
    let ResearchRecord::Artifact(artifact) = ResearchRecord::decode(bytes)? else {
        return Err("expected fixture artifact".into());
    };
    let mut wrapper =
        br#"{"kind":"research.experiment.v1","publication_eligible":false,"result":"#.to_vec();
    wrapper.extend_from_slice(bytes.strip_suffix(b"\n").ok_or("fixture newline")?);
    wrapper.extend_from_slice(b"}\n");
    let result = ExperimentResult::decode(&wrapper)?;
    let promoted = String::from_utf8(wrapper)?.replace(":false", ":true");
    assert!(ExperimentResult::decode(promoted.as_bytes()).is_err());

    let mut alternate = artifact.request.clone();
    alternate.request_id = "trial-02".into();
    alternate.configuration = "04".repeat(32);
    let trial = artifact.request.request_id.clone();
    let command = |action| Command::V1 {
        scope: artifact.request.scope.clone(),
        family: "momentum-study".into(),
        action,
    };
    let registration = command(Action::Register {
        protocol: "01".repeat(32),
        trials: vec![
            Trial {
                request: artifact.request.clone(),
                evidence: artifact.evidence.clone(),
            },
            Trial {
                request: alternate.clone(),
                evidence: artifact.evidence.clone(),
            },
        ],
    });
    let success = command(Action::Finish {
        trial: trial.clone(),
        outcome: Terminal::Result {
            result: result.clone(),
        },
    });
    let failure = command(Action::Finish {
        trial: alternate.request_id.clone(),
        outcome: Terminal::Failed {
            reason: Failure::Unavailable,
        },
    });
    let selection = command(Action::Select {
        trial: trial.clone(),
    });
    let mut ledger = Ledger::default();
    assert!(ledger.apply(&success).is_err());
    assert!(ledger.apply(&registration)?);
    assert!(!ledger.apply(&registration)?);
    assert!(
        ledger
            .apply(&command(Action::Finish {
                trial: alternate.request_id.clone(),
                outcome: Terminal::Result { result }
            }))
            .is_err()
    );
    assert!(ledger.apply(&success)?);
    assert!(ledger.apply(&selection).is_err());
    assert!(ledger.apply(&failure)?);
    assert!(
        ledger
            .apply(&command(Action::Select {
                trial: alternate.request_id
            }))
            .is_err()
    );
    assert!(ledger.apply(&selection)?);
    assert!(!ledger.apply(&success)?);
    assert!(!ledger.apply(&selection)?);
    assert!(
        ledger
            .apply(&command(Action::Finish {
                trial,
                outcome: Terminal::Failed {
                    reason: Failure::InvalidInput
                }
            }))
            .is_err()
    );
    assert_eq!(ledger.entries().len(), 4);
    Ok(())
}
