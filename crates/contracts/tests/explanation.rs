use edgeagent_contracts::{
    explanation::{
        Anchor, Binding, Claim, Companion, Disclosure, Fallback, Job, MAX_BYTES, Outcome, Proposal,
        Refusal, Selection, Variant,
    },
    research::ResearchRecord,
    research_decision::{Assessment, DecisionProvenance, Reason, Status},
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const ARTIFACT: &[u8] = include_bytes!("../fixtures/research/v1/artifact.json");

fn example() -> Result<(Job, Companion, ResearchRecord, DecisionProvenance)> {
    let artifact = ResearchRecord::decode(ARTIFACT)?;
    let decision =
        DecisionProvenance::decode(include_bytes!("../fixtures/research/decision-v1.json"))?;
    let ResearchRecord::Artifact(value) = &artifact else {
        return Err("expected artifact".into());
    };
    let anchor = Anchor {
        scope: value.request.scope.clone(),
        artifact: artifact.digest()?,
        decision: decision.digest()?,
    };
    let claims = value
        .ranking
        .iter()
        .map(|rank| Claim::Ranked {
            instrument: rank.instrument.clone(),
        })
        .chain([Claim::Disclosure {
            disclosure: Disclosure::ResearchOnly,
        }])
        .map(|claim| Selection {
            claim,
            citation: "a".repeat(64),
            variant: Variant::Standard,
        })
        .collect();
    Ok((
        Job {
            anchor: anchor.clone(),
        },
        Companion {
            binding: Binding {
                anchor,
                profile: "b".repeat(64),
                evidence: "c".repeat(64),
            },
            outcome: Outcome::Generated {
                proposal: Proposal { claims },
            },
        },
        artifact,
        decision,
    ))
}

#[test]
fn strict_bounded_proposals() -> Result {
    let (_, companion, _, _) = example()?;
    let Outcome::Generated { mut proposal } = companion.outcome else {
        return Err("expected proposal".into());
    };
    let bytes = proposal.canonical_bytes()?;
    assert_eq!(Proposal::decode(&bytes)?.canonical_bytes()?, bytes);
    let wire = String::from_utf8(bytes)?;
    for invalid in [
        wire.replace("proposal.v1", "proposal.v2"),
        wire.replace("\"claims\":", "\"unknown\":0,\"claims\":"),
        wire.replace("\"citation\":", "\"citation\":\"duplicate\",\"citation\":"),
        wire.replace("\"variant\":", "\"score\":0,\"variant\":"),
        wire.replace(&"a".repeat(64), &"0".repeat(64)),
    ] {
        assert!(Proposal::decode(invalid.as_bytes()).is_err());
    }
    assert!(Proposal::decode(&vec![b' '; MAX_BYTES + 1]).is_err());
    proposal.claims.push(proposal.claims[0].clone());
    assert!(proposal.canonical_bytes().is_err());
    proposal.claims = vec![proposal.claims[0].clone(); 74];
    assert!(proposal.canonical_bytes().is_err());
    Ok(())
}

#[test]
fn binds_coverage_and_preserves_legacy_bytes() -> Result {
    let (job, mut companion, artifact, mut decision) = example()?;
    companion.bind(&artifact, &decision)?;
    job.bind(&artifact, &decision)?;
    assert_eq!(artifact.canonical_bytes()?, ARTIFACT);
    let bytes = companion.canonical_bytes()?;
    assert_eq!(Companion::decode(&bytes)?.canonical_bytes()?, bytes);
    decision.validator = "changed".into();
    assert!(job.bind(&artifact, &decision).is_err());
    let decision =
        DecisionProvenance::decode(include_bytes!("../fixtures/research/decision-v1.json"))?;
    let Outcome::Generated { proposal } = &mut companion.outcome else {
        return Err("expected proposal".into());
    };
    proposal.claims.swap(0, 1);
    assert!(companion.bind(&artifact, &decision).is_err());

    let (_, mut narrowed, mut artifact, mut decision) = example()?;
    let ResearchRecord::Artifact(value) = &mut artifact else {
        return Err("expected artifact".into());
    };
    value.request.instruments.push("BBB".into());
    decision.request = value.request.clone();
    decision.outcome = artifact.digest()?;
    decision.assessment = Assessment::Evaluated {
        status: Status::Narrowed,
        eligible: vec!["AAA".into()],
        reasons: vec![Reason::LiquidityBelowMinimum],
    };
    narrowed.binding.anchor.artifact = artifact.digest()?;
    narrowed.binding.anchor.decision = decision.digest()?;
    assert!(narrowed.bind(&artifact, &decision).is_err());
    let Outcome::Generated { proposal } = &mut narrowed.outcome else {
        return Err("expected proposal".into());
    };
    proposal.claims.insert(
        1,
        Selection {
            claim: Claim::PolicyReason {
                reason: Reason::LiquidityBelowMinimum,
            },
            citation: "d".repeat(64),
            variant: Variant::Standard,
        },
    );
    narrowed.bind(&artifact, &decision)?;
    Ok(())
}

#[test]
fn terminal_variants_and_identities_are_distinct() -> Result {
    let (job, generated, artifact, decision) = example()?;
    let Outcome::Generated { proposal } = generated.outcome.clone() else {
        return Err("expected proposal".into());
    };
    let fallback = Companion {
        binding: generated.binding.clone(),
        outcome: Outcome::Fallback {
            reason: Fallback::Timeout,
            proposal,
        },
    };
    let refused = Companion {
        binding: generated.binding.clone(),
        outcome: Outcome::Refused {
            reason: Refusal::DisclosureDenied,
        },
    };
    for value in [&generated, &fallback, &refused] {
        value.bind(&artifact, &decision)?;
        assert_eq!(value.job().id()?, job.id()?);
        let bytes = value.canonical_bytes()?;
        assert_eq!(Companion::decode(&bytes)?.canonical_bytes()?, bytes);
    }
    assert_ne!(generated.id()?, fallback.id()?);
    assert_ne!(fallback.id()?, refused.id()?);
    assert_eq!(Job::decode(&job.canonical_bytes()?)?.id()?, job.id()?);
    Ok(())
}

#[test]
fn frozen_wire_identities() -> Result {
    let (job, companion, _, _) = example()?;
    let Outcome::Generated { proposal } = &companion.outcome else {
        return Err("expected proposal".into());
    };
    assert_eq!(
        job.canonical_bytes()?,
        include_bytes!("../fixtures/explanation/job-v1.json")
    );
    assert_eq!(
        companion.canonical_bytes()?,
        include_bytes!("../fixtures/explanation/companion-v1.json")
    );
    assert_eq!(
        proposal.canonical_bytes()?,
        include_bytes!("../fixtures/explanation/proposal-v1.json")
    );
    assert_eq!(
        edgeagent_contracts::explanation::RECORDED.message_type(),
        "com.edgeagent.research.explanation-recorded.v1"
    );
    Ok(())
}

#[cfg(feature = "schema")]
#[test]
fn schemas_match_frozen_v1_inventory() -> Result {
    assert_eq!(
        Job::schema()?,
        include_bytes!("../fixtures/explanation/job-v1.schema.json")
    );
    assert_eq!(
        Companion::schema()?,
        include_bytes!("../fixtures/explanation/companion-v1.schema.json")
    );
    assert_eq!(
        Proposal::schema()?,
        include_bytes!("../fixtures/explanation/proposal-v1.schema.json")
    );
    Ok(())
}
