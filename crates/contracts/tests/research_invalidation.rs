use edgeagent_contracts::{
    research::ResearchRecord,
    research_invalidation::{Command, MAX_COMMAND_BYTES},
};
#[test]
fn invalidation_commands_are_canonical_bounded_and_reference_only_retained_requests()
-> Result<(), Box<dyn std::error::Error>> {
    let ResearchRecord::Artifact(base) =
        ResearchRecord::decode(include_bytes!("../fixtures/research/v1/artifact.json"))?
    else {
        return Err("artifact".into());
    };
    let original = base.request;
    let mut next = original.clone();
    next.request_id = "later".into();
    next.decision_at_ms += 1;
    let command = Command::V1 {
        original,
        effective_at_ms: 102,
        reassessment: Some(next),
    };
    let bytes = command.bytes()?;
    assert!(Command::decode(&bytes)? == command);
    assert!(Command::decode(&bytes[..bytes.len() - 1]).is_err());
    assert!(Command::decode(&vec![b' '; MAX_COMMAND_BYTES + 1]).is_err());
    let text = std::str::from_utf8(&bytes)?;
    for invalid in [
        text.replace(
            "research.invalidation-check.v1",
            "research.invalidation-check.v2",
        ),
        text.replace(
            "\"effective_at_ms\":102",
            "\"effective_at_ms\":102,\"actor\":\"private-sentinel\"",
        ),
        text.replace(
            "\"effective_at_ms\":102",
            "\"effective_at_ms\":102,\"effective_at_ms\":102",
        ),
    ] {
        let e = Command::decode(invalid.as_bytes())
            .err()
            .ok_or("must refuse")?;
        assert!(!format!("{e:?} {e}").contains("private-sentinel"));
    }
    let Command::V1 {
        original,
        reassessment,
        ..
    } = command;
    let mut wrong = reassessment.ok_or("next")?;
    wrong.configuration = "09".repeat(32);
    assert!(
        Command::V1 {
            original,
            effective_at_ms: 102,
            reassessment: Some(wrong)
        }
        .bytes()
        .is_err()
    );
    Ok(())
}
