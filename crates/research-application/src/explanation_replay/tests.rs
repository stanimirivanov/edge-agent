use super::*;
use edgeagent_contracts::{
    evidence::DailyEvidenceArtifact,
    explanation::{Fallback, Refusal, Variant},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn inputs() -> Result<(ResearchRecord, DecisionProvenance, Profile), Box<dyn std::error::Error>> {
    Ok((
        ResearchRecord::decode(include_bytes!(
            "../../../contracts/fixtures/research/v1/artifact.json"
        ))?,
        DecisionProvenance::decode(include_bytes!(
            "../../../contracts/fixtures/research/decision-v1.json"
        ))?,
        serde_json::from_value(serde_json::json!({
            "deployment":{"runtime":"a".repeat(64),"weights":"b".repeat(64)},
            "settings":{"seed":1,"temperature_milli":0,"max_output_tokens":512,"timeout_ms":1000},
            "prompt":"c".repeat(64), "tools":"none",
            "validator":DailyEvidenceArtifact::hash(b"research-explanation-allowlist-v1").as_str(),
            "renderer":DailyEvidenceArtifact::hash(b"research-explanation-trace.v1").as_str()
        }))?,
    ))
}
fn companion(
    record: &ResearchRecord,
    decision: &DecisionProvenance,
    profile: &Profile,
) -> Result<Companion, Box<dyn std::error::Error>> {
    let prepared = Prepared::new(record, decision, profile)?;
    Ok(Companion {
        binding: prepared.binding().clone(),
        outcome: Outcome::Fallback {
            reason: Fallback::Disabled,
            proposal: prepared.disabled()?.explanation.proposal().clone(),
        },
    })
}
#[test]
fn replay_renders_the_retained_proposal_and_requires_standard_fallback() -> TestResult {
    let (record, decision, profile) = inputs()?;
    let mut retained = companion(&record, &decision, &profile)?;
    let standard = validate(
        &record,
        &decision,
        Retained {
            profile: &profile,
            companion: &retained.canonical_bytes()?,
        },
    )?;
    let Outcome::Fallback { mut proposal, .. } = retained.outcome else {
        return Err("fallback".into());
    };
    proposal.claims[0].variant = Variant::PlainLanguage;
    retained.outcome = Outcome::Generated {
        proposal: proposal.clone(),
    };
    let generated = validate(
        &record,
        &decision,
        Retained {
            profile: &profile,
            companion: &retained.canonical_bytes()?,
        },
    )?;
    assert!(generated.rendered().contains("Rank 1:"));
    assert_ne!(generated.rendered(), standard.rendered());
    let mut unsupported = proposal.clone();
    unsupported.claims[0].citation = "e".repeat(64);
    retained.outcome = Outcome::Generated {
        proposal: unsupported,
    };
    assert!(
        validate(
            &record,
            &decision,
            Retained {
                profile: &profile,
                companion: &retained.canonical_bytes()?
            }
        )
        .is_err()
    );
    for reason in [
        Fallback::Disabled,
        Fallback::Unavailable,
        Fallback::Timeout,
        Fallback::BudgetExhausted,
        Fallback::InvalidProposal,
        Fallback::UncertainAttempt,
    ] {
        retained.outcome = Outcome::Fallback {
            reason,
            proposal: standard.proposal().clone(),
        };
        assert_eq!(
            validate(
                &record,
                &decision,
                Retained {
                    profile: &profile,
                    companion: &retained.canonical_bytes()?
                }
            )?
            .rendered(),
            standard.rendered()
        );
    }
    Ok(())
}
#[test]
fn replay_rejects_changed_citations_bindings_profiles_and_retained_encoding() -> TestResult {
    let (record, decision, profile) = inputs()?;
    let original = companion(&record, &decision, &profile)?.canonical_bytes()?;
    for field in [
        "profile", "evidence", "artifact", "decision", "scope", "citation", "variant",
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(&original)?;
        match field {
            "profile" | "evidence" => value["data"]["binding"][field] = "e".repeat(64).into(),
            "artifact" | "decision" | "scope" => {
                value["data"]["binding"]["anchor"][field] = "e".repeat(64).into()
            }
            "citation" => {
                value["data"]["outcome"]["proposal"]["claims"][0][field] = "e".repeat(64).into()
            }
            _ => {
                value["data"]["outcome"]["proposal"]["claims"][0][field] = "unknown-template".into()
            }
        }
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        assert!(
            validate(
                &record,
                &decision,
                Retained {
                    profile: &profile,
                    companion: &bytes
                }
            )
            .is_err(),
            "accepted {field}"
        );
    }
    for field in ["prompt", "renderer", "validator"] {
        let mut changed = serde_json::to_value(&profile)?;
        changed[field] = "e".repeat(64).into();
        let changed = serde_json::from_value(changed)?;
        assert!(
            validate(
                &record,
                &decision,
                Retained {
                    profile: &changed,
                    companion: &original
                }
            )
            .is_err()
        );
    }
    let mut noncanonical = original.clone();
    noncanonical.push(b'\n');
    assert!(
        validate(
            &record,
            &decision,
            Retained {
                profile: &profile,
                companion: &noncanonical
            }
        )
        .is_err()
    );
    let mut refused = companion(&record, &decision, &profile)?;
    refused.outcome = Outcome::Refused {
        reason: Refusal::DisclosureDenied,
    };
    assert!(
        validate(
            &record,
            &decision,
            Retained {
                profile: &profile,
                companion: &refused.canonical_bytes()?
            }
        )
        .is_err()
    );
    Ok(())
}
