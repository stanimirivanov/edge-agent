use edgeagent_contracts::{
    evidence::DailyEvidenceArtifact, explanation_facts::Facts, research::ResearchRecord,
    research_decision::DecisionProvenance,
};
use edgeagent_research_application::{
    explanation::Allowlist, explanation_preparation::CanonicalPreparation,
};
use serde_json::{Value, json};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn fixture() -> Result<(ResearchRecord, DecisionProvenance)> {
    Ok((
        ResearchRecord::decode(include_bytes!(
            "../../contracts/fixtures/research/v1/artifact.json"
        ))?,
        DecisionProvenance::decode(include_bytes!(
            "../../contracts/fixtures/research/decision-v1.json"
        ))?,
    ))
}

#[test]
fn facts_preserve_retained_intent_units_times_and_source_without_invention() -> Result {
    let (mut record, mut decision) = fixture()?;
    if let ResearchRecord::Artifact(a) = &mut record {
        a.request.instruments.push("BBB".into());
        a.evidence.push(edgeagent_contracts::research::EvidenceRef {
            instrument: "BBB".into(),
            digest: "d".repeat(64),
            available_at_ms: 91,
        });
        decision.request = a.request.clone();
    }
    decision.outcome = record.digest()?;
    decision.assessment = edgeagent_contracts::research_decision::Assessment::Evaluated {
        status: edgeagent_contracts::research_decision::Status::Narrowed,
        eligible: vec!["AAA".into()],
        reasons: vec![edgeagent_contracts::research_decision::Reason::InstrumentNotAllowed],
    };
    let facts = Facts::from_bound(&record, &decision)?;
    let value: Value = serde_json::from_slice(&facts.canonical_bytes()?)?;
    assert_eq!(value["kind"], "research.explanation-facts.v1");
    assert_eq!(value["anchor"]["artifact"], record.digest()?);
    assert_eq!(value["anchor"]["decision"], decision.digest()?);
    assert_eq!(value["request"]["configuration"], "b".repeat(64));
    assert_eq!(value["request"]["decision_at_ms"], 100);
    assert_eq!(value["expires_at_ms"], 200);
    assert_eq!(value["evidence"].as_array().map(Vec::len), Some(2));
    assert_eq!(value["evidence"][1]["instrument"], "BBB");
    assert_eq!(
        value["ranks"][0]["score"],
        json!({"unit":"ppm", "value":10000})
    );
    assert_eq!(value["ranks"][0]["evidence"][0]["available_at_ms"], 90);
    assert_eq!(value["ranks"][0]["evidence"][0]["digest"], "c".repeat(64));
    assert_eq!(
        value["decision"]["data"]["horizon_sessions"],
        decision.horizon_sessions
    );
    assert_eq!(
        value["decision"]["data"]["approval_profile"],
        decision.approval_profile
    );
    // These facts do not exist in a retained v1 artifact: absence must not become zero.
    for absent in ["event_at_ms", "observed_at_ms", "liquidity", "volatility"] {
        assert!(!String::from_utf8(facts.canonical_bytes()?)?.contains(absent));
    }
    assert!(!String::from_utf8(facts.canonical_bytes()?)?.contains("ranks first"));
    Ok(())
}

#[test]
fn canonical_input_is_exactly_bound_and_does_not_accept_model_facts() -> Result {
    let (record, decision) = fixture()?;
    let prepared = CanonicalPreparation::from_verified(&record, &decision)?;
    let value: Value = serde_json::from_slice(prepared.input())?;
    assert_eq!(value["kind"], "research.explanation-input.v2");
    assert_eq!(value["untrusted_context"], Value::Null);
    let standard = Allowlist::from_verified(&record, &decision)?.standard()?;
    assert_eq!(
        value["standard_proposal"],
        serde_json::from_slice::<Value>(&standard.proposal().canonical_bytes()?)?
    );
    assert!(prepared.anchor() == standard.anchor());
    assert_eq!(
        prepared.input_digest(),
        DailyEvidenceArtifact::hash(prepared.input()).as_str()
    );
    assert_eq!(
        prepared.input(),
        CanonicalPreparation::from_verified(&record, &decision)?.input()
    );
    assert_eq!(
        value["facts"],
        serde_json::from_slice::<Value>(&prepared.facts().canonical_bytes()?)?
    );
    assert!(prepared.input().starts_with(b"{\"facts\":{\"anchor\":"));
    assert!(prepared.input().ends_with(b"\"untrusted_context\":null}\n"));
    // A changed retained fact requires a new decision, anchor and prepared identity.
    let mut changed = record.clone();
    if let ResearchRecord::Artifact(a) = &mut changed {
        a.ranking[0].score_ppm += 1;
    }
    assert!(CanonicalPreparation::from_verified(&changed, &decision).is_err());
    let mut rebound = decision.clone();
    rebound.outcome = changed.digest()?;
    assert_ne!(
        prepared.input_digest(),
        CanonicalPreparation::from_verified(&changed, &rebound)?.input_digest()
    );
    Ok(())
}

#[test]
fn missing_rank_evidence_and_foreign_decisions_fail_closed_even_after_rebinding() -> Result {
    let (mut record, mut decision) = fixture()?;
    decision.request.configuration = "e".repeat(64);
    assert!(Facts::from_bound(&record, &decision).is_err());
    let (_, mut decision) = fixture()?;
    if let ResearchRecord::Artifact(a) = &mut record {
        a.evidence.clear();
    }
    // Contract-level shape validation can also reject the incomplete record.
    if let Ok(digest) = record.digest() {
        decision.outcome = digest;
    }
    assert!(Facts::from_bound(&record, &decision).is_err());
    assert!(CanonicalPreparation::from_verified(&record, &decision).is_err());
    let abstention = ResearchRecord::decode(include_bytes!(
        "../../contracts/fixtures/research/v1/abstention.json"
    ))?;
    assert!(Facts::from_bound(&abstention, &decision).is_err());
    Ok(())
}

#[test]
fn model_supplied_facts_are_refused_and_rank_source_order_is_preserved() -> Result {
    use edgeagent_contracts::research::{EvidenceRef, RankedCandidate};
    let (mut record, mut decision) = fixture()?;
    if let ResearchRecord::Artifact(a) = &mut record {
        a.request.instruments.push("BBB".into());
        a.ranking.insert(
            0,
            RankedCandidate {
                instrument: "BBB".into(),
                score_ppm: 20000,
            },
        );
        a.evidence.push(EvidenceRef {
            instrument: "BBB".into(),
            digest: "f".repeat(64),
            available_at_ms: 95,
        });
    }
    if let ResearchRecord::Artifact(a) = &record {
        decision.request = a.request.clone();
    }
    decision.outcome = record.digest()?;
    if let edgeagent_contracts::research_decision::Assessment::Evaluated { eligible, .. } =
        &mut decision.assessment
    {
        eligible.push("BBB".into());
    }
    let prepared = CanonicalPreparation::from_verified(&record, &decision)?;
    let value: Value = serde_json::from_slice(prepared.input())?;
    assert_eq!(value["facts"]["ranks"][0]["instrument"], "BBB");
    assert_eq!(value["facts"]["ranks"][1]["instrument"], "AAA");
    assert_eq!(
        value["facts"]["ranks"][0]["evidence"][0]["digest"],
        "f".repeat(64)
    );
    let mut forged = value["standard_proposal"].clone();
    forged["facts"] = json!({"score": 999999});
    let mut bytes = serde_json::to_vec(&forged)?;
    bytes.push(b'\n');
    assert!(
        Allowlist::from_verified(&record, &decision)?
            .validate(&bytes)
            .is_err()
    );
    // Pinned across default and preserve_order feature configurations.
    assert_eq!(
        prepared.input_digest(),
        "0bf1641d333cd65ef0465a807af48f271bbc53aa72d0d43e6d97058e71d93657"
    );
    Ok(())
}
