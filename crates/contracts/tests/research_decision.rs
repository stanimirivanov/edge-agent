//! Companion compatibility and safe untrusted-boundary proofs.
use edgeagent_contracts::{
    research::{AbstentionReason, ResearchRecord},
    research_decision::{Assessment, DecisionProvenance, MAX_DECISION_BYTES, Status},
};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn fixture() -> Result<DecisionProvenance> {
    Ok(DecisionProvenance::decode(include_bytes!(
        "../fixtures/research/decision-v1.json"
    ))?)
}
fn artifact() -> Result<ResearchRecord> {
    Ok(ResearchRecord::decode(include_bytes!(
        "../fixtures/research/v1/artifact.json"
    ))?)
}
#[test]
fn frozen_companion_preserves_legacy_artifact_and_rejects_noncanonical_versions() -> Result {
    let record = fixture()?;
    let outcome = artifact()?;
    record.bind(&outcome)?;
    assert_eq!(
        record.canonical_bytes()?,
        include_bytes!("../fixtures/research/decision-v1.json")
    );
    assert_eq!(
        outcome.canonical_bytes()?,
        include_bytes!("../fixtures/research/v1/artifact.json")
    );
    assert_eq!(record.digest()?.len(), 64);
    let original: serde_json::Value = serde_json::from_slice(&record.canonical_bytes()?)?;
    for field in 0..9 {
        let mut value = original.clone();
        match field {
            0 => value["kind"] = "research.decision.v2".into(),
            1 => value["unknown"] = "PRIVATE_SENTINEL".into(),
            2 => {
                value["data"]
                    .as_object_mut()
                    .ok_or("object")?
                    .remove("policy");
            }
            3 => value["data"]["assessment"]["unknown"] = true.into(),
            4 => value["data"]["approval_revision"] = 0.into(),
            5 => value["data"]["outcome"] = "0".repeat(64).into(),
            6 => value["data"]["horizon_sessions"] = 253.into(),
            7 => value["data"]["assessment"]["status"] = "narrowed".into(),
            _ => value["data"]["assessment"]["reasons"] = serde_json::json!(["unknown"]),
        }
        let mut invalid = serde_json::to_vec(&value)?;
        invalid.push(b'\n');
        let error = DecisionProvenance::decode(&invalid)
            .err()
            .ok_or("accepted invalid")?;
        assert!(!format!("{error:?} {error}").contains("PRIVATE_SENTINEL"));
    }
    let bytes = record.canonical_bytes()?;
    assert!(DecisionProvenance::decode(&bytes[..bytes.len() - 1]).is_err());
    let duplicate = String::from_utf8(bytes.clone())?.replacen(
        "\"kind\":",
        "\"kind\":\"research.decision.v1\",\"kind\":",
        1,
    );
    assert!(DecisionProvenance::decode(duplicate.as_bytes()).is_err());
    assert!(DecisionProvenance::decode(&vec![0; MAX_DECISION_BYTES + 1]).is_err());
    Ok(())
}
#[test]
fn assessments_bind_exact_terminal_kind_universe_and_reason_semantics() -> Result {
    let mut record = fixture()?;
    let artifact = artifact()?;
    let mut abstention =
        ResearchRecord::Abstention(edgeagent_contracts::research::ResearchAbstention {
            request: record.request.clone(),
            reason: AbstentionReason::MissingEntitlement,
        });
    record.outcome = abstention.digest()?;
    assert!(record.bind(&abstention).is_err());
    record.assessment = Assessment::NotEvaluated {
        reason: AbstentionReason::MissingEntitlement,
    };
    record.bind(&abstention)?;
    assert!(record.bind(&artifact).is_err());
    record.assessment = Assessment::NotEvaluated {
        reason: AbstentionReason::NoEligibleCandidate,
    };
    assert!(record.canonical_bytes().is_err());
    if let ResearchRecord::Abstention(a) = &mut abstention {
        a.reason = AbstentionReason::NoEligibleCandidate;
    }
    record.outcome = abstention.digest()?;
    record.assessment = Assessment::Evaluated {
        status: Status::Abstained,
        eligible: vec![],
        reasons: vec![edgeagent_contracts::research_decision::Reason::DirectionNotAllowed],
    };
    record.bind(&abstention)?;
    for field in 0..4 {
        let mut changed = record.clone();
        if let Assessment::Evaluated {
            status,
            eligible,
            reasons,
        } = &mut changed.assessment
        {
            match field {
                0 => *status = Status::Approved,
                1 => eligible.push("FOREIGN".into()),
                2 => reasons.push(reasons[0]),
                _ => reasons.clear(),
            }
        }
        assert!(changed.canonical_bytes().is_err());
    }
    // A nonempty narrowed result preserves the original universe and matches only its ranking.
    let mut narrowed = artifact.clone();
    if let ResearchRecord::Artifact(a) = &mut narrowed {
        a.request.instruments.push("BBB".into());
        record.request = a.request.clone();
    }
    record.outcome = narrowed.digest()?;
    record.assessment = Assessment::Evaluated {
        status: Status::Narrowed,
        eligible: vec!["AAA".into()],
        reasons: vec![edgeagent_contracts::research_decision::Reason::InstrumentNotAllowed],
    };
    record.bind(&narrowed)?;
    let narrowed_bytes = record.canonical_bytes()?;
    DecisionProvenance::decode(&narrowed_bytes)?.bind(&narrowed)?;
    if let Assessment::Evaluated { eligible, .. } = &mut record.assessment {
        *eligible = vec!["BBB".into()];
    }
    // Still a structurally valid subset, but inconsistent with the exact ranking.
    record.canonical_bytes()?;
    assert!(record.bind(&narrowed).is_err());
    // A canonical but different terminal record cannot inherit the decision.
    let mut changed = artifact.clone();
    if let ResearchRecord::Artifact(a) = &mut changed {
        a.ranking[0].score_ppm += 1;
    }
    assert!(fixture()?.bind(&changed).is_err());
    Ok(())
}
