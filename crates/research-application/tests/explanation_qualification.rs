#[path = "support/qualification.rs"]
mod fixture;
use edgeagent_contracts::explanation_qualification::*;
use edgeagent_research_application::explanation_qualification::VerifiedReview;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[test]
fn complete_scoring_is_required_before_sealing_an_exact_profile() -> Result {
    let command = fixture::approved()?;
    let bytes = command.bytes()?;
    let verified = VerifiedReview::decode(&bytes)?;
    assert_eq!(verified.command().profile().id()?, command.profile().id()?);
    assert_eq!(verified.assessment().len(), 64);
    assert_eq!(verified.collection().len(), 64);
    assert_eq!(
        VerifiedReview::decode(&bytes)?.assessment(),
        verified.assessment()
    );
    for field in 0..14 {
        let mut wrong = command.clone();
        let Command::Approve { evidence, .. } = &mut wrong else {
            return Err("approve".into());
        };
        match field {
            0 => {
                evidence.collection.suite =
                    edgeagent_research_application::explanation_evaluation::Suite::frozen()?
                        .identity()
                        .into()
            }
            1 => evidence.collection.preparation = "a".repeat(64),
            2 => {
                evidence.collection.cases.pop();
            }
            3 => evidence.reviews[1].reviewer = evidence.reviews[0].reviewer.clone(),
            4 => evidence.reviews[0].ratings[0].presentation = "a".repeat(64),
            5 => evidence.collection.profile.settings.seed = 2,
            6 => evidence.collection.profile.renderer = "a".repeat(64),
            7 => evidence.collection.profile.validator = "a".repeat(64),
            8 => {
                for rating in &mut evidence.reviews[0].ratings {
                    rating.preference = Preference::Tie;
                }
            }
            10 => {
                evidence.collection.safety.pop();
            }
            11 => evidence.collection.safety[0].candidate = Observation::Failed,
            12 => evidence.collection.cases[0].input = "a".repeat(64),
            13 => {
                evidence.reviews[0].ratings.pop();
            }
            _ => {
                for result in &mut evidence.collection.cases {
                    result.candidate = Observation::Failed;
                }
            }
        }
        assert!(
            VerifiedReview::decode(&wrong.bytes()?).is_err(),
            "bad evidence {field}"
        );
    }
    let text = String::from_utf8(bytes)?;
    for invalid in [
        text.replace("owned_local_runtime_v1", "synthetic_process_v1"),
        text.replace("review.v1", "review.v0"),
        text.replace("\"revision\":1", "\"revision\":1,\"revision\":1"),
        text.replace("\"revision\":1", "\"passed\":true,\"revision\":1"),
        format!(" {text}"),
    ] {
        assert!(VerifiedReview::decode(invalid.as_bytes()).is_err());
    }
    assert!(VerifiedReview::decode(&vec![b' '; MAX_COMMAND_BYTES + 1]).is_err());
    Ok(())
}
#[test]
fn evidence_identity_binds_private_artifacts_and_owner_attestation() -> Result {
    let original = fixture::approved()?;
    let first = VerifiedReview::decode(&original.bytes()?)?;
    assert_eq!(
        first.collection(),
        "5ebdc796ef31f3cd3ba10109f1af238553d8f08f800d99aac42b43f5918debf1"
    );
    assert_eq!(
        first.assessment(),
        "82666091e8cce6dbbe9152a804f5af4fa016c6efe6d0a57cdc45a589c2ebcbb7"
    );
    for field in 0..3 {
        let mut changed = original.clone();
        let Command::Approve {
            attestation,
            evidence,
            ..
        } = &mut changed
        else {
            return Err("approve".into());
        };
        match field {
            0 => evidence.collection.manifest = "a".repeat(64),
            1 => evidence.collection.helper = "a".repeat(64),
            _ => *attestation = "a".repeat(64),
        };
        let next = VerifiedReview::decode(&changed.bytes()?)?;
        assert_ne!(next.bytes(), first.bytes());
        if field < 2 {
            assert_ne!(next.collection(), first.collection());
            assert_ne!(next.assessment(), first.assessment());
        }
    }
    Ok(())
}

#[test]
fn qualification_keeps_the_historical_context_corpus_unchanged() {
    assert_eq!(
        include_str!("fixtures/explanations/model-context-v1.tsv"),
        include_str!("../../../services/research/fixtures/explanations/model-context-v1.tsv")
    );
}
