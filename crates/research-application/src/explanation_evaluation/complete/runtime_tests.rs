use super::*;
use crate::explanation_runtime::{
    INPUT_REVISION, ModelContext, RENDERER_REVISION, RuntimePreparation, ServingContext,
    VALIDATOR_REVISION,
};
use edgeagent_contracts::explanation_profile::{Deployment, Profile, Settings, Tools};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn profile() -> Profile {
    Profile {
        deployment: Deployment {
            runtime: "a".repeat(64),
            weights: "b".repeat(64),
        },
        settings: Settings {
            seed: 1,
            temperature_milli: 0,
            max_output_tokens: 512,
            timeout_ms: 1000,
        },
        prompt: "c".repeat(64),
        validator: edgeagent_contracts::evidence::DailyEvidenceArtifact::hash(
            VALIDATOR_REVISION.as_bytes(),
        )
        .as_str()
        .into(),
        renderer: edgeagent_contracts::evidence::DailyEvidenceArtifact::hash(
            RENDERER_REVISION.as_bytes(),
        )
        .as_str()
        .into(),
        tools: Tools::None,
    }
}
#[test]
fn runtime_suite_uses_the_same_verified_preparation_and_rendering_for_every_task() -> Result {
    let suite = Suite::runtime()?;
    let old = Suite::complete()?;
    assert_eq!(
        old.identity(),
        "f74eebaddbb3ce2553ba5139d72aaa75eb4836bee8e7ef6687911f64ba85e2a0"
    );
    assert_ne!(suite.identity(), old.identity());
    assert_eq!(suite.cases().len(), 30);
    for (index, (case, historical)) in suite.cases().iter().zip(old.cases()).enumerate() {
        assert_eq!(
            (case.id(), case.task(), case.scenario()),
            (historical.id(), historical.task(), historical.scenario())
        );
        let f = recipe::build(index + 1)?;
        let input = case.input().ok_or("input")?;
        let bytes = input.prepared_input().ok_or("prepared")?;
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        assert_eq!(value["kind"], INPUT_REVISION);
        assert!(value.get("assertions").is_none());
        assert!(!String::from_utf8_lossy(bytes).contains("invented-selected-daily-v1"));
        if index == 28 {
            continue;
        }
        let runtime = RuntimePreparation::from_verified(
            &f.record,
            &f.decision,
            &f.configuration,
            &f.input,
            None,
        )?;
        assert_eq!(runtime.input(), bytes);
        assert_eq!(runtime.standard(None)?.rendered(), input.baseline());
        let binding = runtime.binding(&profile())?;
        assert_eq!(binding.evidence, runtime.input_digest());
        assert_eq!(binding.profile, profile().id()?);
        let mut wrong = profile();
        wrong.renderer = "a".repeat(64);
        assert!(runtime.binding(&wrong).is_err());
        let mut wrong = profile();
        wrong.validator = "a".repeat(64);
        assert!(runtime.binding(&wrong).is_err());
        let mut plain = Proposal::decode(input.standard_proposal())?;
        plain.claims[0].variant = Variant::PlainLanguage;
        assert_eq!(
            runtime
                .validate(&plain.canonical_bytes()?, None)?
                .rendered(),
            input.render_candidate(&plain.canonical_bytes()?)?
        );
        plain.claims[0].citation = "f".repeat(64);
        assert!(runtime.validate(&plain.canonical_bytes()?, None).is_err());
    }
    Ok(())
}
#[test]
fn runtime_context_requires_current_model_and_serving_permissions() -> Result {
    let f = recipe::build(29)?;
    let doc = DocumentExcerpt {
        source: "invented-reviewed-release".into(),
        scope: "evaluation".into(),
        available_at_ms: 100,
        text: "Invented reviewed context. It is not the source of the ranking facts.".into(),
    };
    let bytes = doc.canonical_bytes()?;
    let model = Request {
        anchor: Allowlist::from_verified(&f.record, &f.decision)?
            .standard()?
            .anchor()
            .clone(),
        audience: "runtime-evaluation".into(),
        purpose: Purpose::ModelDisclosure,
        document: Some(doc.id()?),
    };
    let artifact = rights(&Request {
        document: None,
        ..model.clone()
    });
    let document = rights(&model);
    let context = || ModelContext {
        bytes: &bytes,
        operation: &model,
        artifact: &artifact,
        document: &document,
        now_ms: 100,
    };
    let runtime = RuntimePreparation::from_verified(
        &f.record,
        &f.decision,
        &f.configuration,
        &f.input,
        Some(context()),
    )?;
    assert!(runtime.standard(None).is_err());
    let serving = Request {
        purpose: Purpose::ExplanationServing,
        ..model.clone()
    };
    let a = rights(&Request {
        document: None,
        ..serving.clone()
    });
    let d = rights(&serving);
    let observed = || ServingContext {
        operation: &serving,
        artifact: &a,
        document: &d,
        now_ms: 100,
    };
    let text = runtime.standard(Some(observed()))?;
    assert!(text.rendered().contains("context_digest="));
    assert!(text.rendered().contains("turnover_cents="));
    let suite = Suite::runtime()?;
    let input = suite.cases()[28].input().ok_or("input")?;
    assert_eq!(runtime.input(), input.prepared_input().ok_or("prepared")?);
    assert_eq!(text.rendered(), input.baseline());
    assert!(
        runtime
            .standard(Some(ServingContext {
                now_ms: 110,
                ..observed()
            }))
            .is_err()
    );
    let revoked = Permissions {
        owner: None,
        research: document.research.clone(),
    };
    assert!(
        RuntimePreparation::from_verified(
            &f.record,
            &f.decision,
            &f.configuration,
            &f.input,
            Some(ModelContext {
                document: &revoked,
                ..context()
            })
        )
        .is_err()
    );
    assert!(
        runtime
            .standard(Some(ServingContext {
                operation: &model,
                ..observed()
            }))
            .is_err()
    );
    let mut malformed = f.configuration.clone();
    malformed.push(b' ');
    assert!(
        RuntimePreparation::from_verified(&f.record, &f.decision, &malformed, &f.input, None)
            .is_err()
    );
    Ok(())
}

#[test]
fn runtime_input_refuses_oversized_combined_verified_data() -> Result {
    let mut f = recipe::build(3)?;
    for input in &mut f.input.context {
        let edgeagent_research_domain::context_policy::Coverage::Complete { events, .. } =
            &mut input.coverage
        else {
            return Err("coverage".into());
        };
        *events = (0..=90).collect();
    }
    assert!(
        PolicyPreview::from_verified(&f.record, &f.decision, &f.configuration, &f.input).is_ok()
    );
    let doc = DocumentExcerpt {
        source: "large-reviewed-release".into(),
        scope: "evaluation".into(),
        available_at_ms: 100,
        text: "\\".repeat(edgeagent_contracts::explanation::MAX_DOCUMENT_TEXT_BYTES),
    };
    let bytes = doc.canonical_bytes()?;
    let model = Request {
        anchor: Allowlist::from_verified(&f.record, &f.decision)?
            .standard()?
            .anchor()
            .clone(),
        audience: "runtime-evaluation".into(),
        purpose: Purpose::ModelDisclosure,
        document: Some(doc.id()?),
    };
    let artifact = rights(&Request {
        document: None,
        ..model.clone()
    });
    let document = rights(&model);
    assert!(
        ReviewedContext::from_verified(
            &f.record,
            &f.decision,
            &bytes,
            &model,
            &artifact,
            &document,
            100
        )
        .is_ok()
    );
    assert!(
        RuntimePreparation::from_verified(
            &f.record,
            &f.decision,
            &f.configuration,
            &f.input,
            Some(ModelContext {
                bytes: &bytes,
                operation: &model,
                artifact: &artifact,
                document: &document,
                now_ms: 100
            })
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn runtime_revision_has_distinct_protocol_and_fresh_presentation_bindings() -> Result {
    use crate::explanation_evaluation::{
        Candidate, CaseResult, Disposition, Preference, Rating, Review,
    };
    let suite = Suite::runtime()?;
    let deployment = profile().id()?;
    let proposals = suite
        .cases()
        .iter()
        .map(|c| {
            let mut p = Proposal::decode(c.input().ok_or("input")?.standard_proposal())?;
            p.claims[0].variant = Variant::PlainLanguage;
            Ok(p.canonical_bytes()?)
        })
        .collect::<Result<Vec<_>>>()?;
    let results = suite
        .cases()
        .iter()
        .zip(&proposals)
        .map(|(c, p)| {
            Ok(CaseResult {
                id: c.id(),
                input: c.input().ok_or("input")?.identity(),
                candidate_first: true,
                candidate: Candidate::Proposed(p),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let presentations = suite
        .cases()
        .iter()
        .zip(&proposals)
        .map(|(c, p)| {
            Ok(c.input()
                .ok_or("input")?
                .presentation_id(&deployment, Candidate::Proposed(p), true))
        })
        .collect::<Result<Vec<_>>>()?;
    let ratings = suite
        .cases()
        .iter()
        .zip(&presentations)
        .enumerate()
        .map(|(n, (c, p))| Rating {
            id: c.id(),
            presentation: p,
            preference: if n < 21 {
                Preference::First
            } else {
                Preference::Tie
            },
        })
        .collect::<Vec<_>>();
    let report = suite.evaluate(
        &deployment,
        &results,
        [
            Review {
                reviewer: "a",
                ratings: &ratings,
            },
            Review {
                reviewer: "b",
                ratings: &ratings,
            },
        ],
    )?;
    assert_eq!(report.disposition, Disposition::Passed);
    assert_eq!(report.preferences, [21, 21]);
    assert_eq!(report.safety.len(), 47);
    let mut insufficient = ratings.clone();
    insufficient[20].preference = Preference::Tie;
    assert_eq!(
        suite
            .evaluate(
                &deployment,
                &results,
                [
                    Review {
                        reviewer: "a",
                        ratings: &insufficient
                    },
                    Review {
                        reviewer: "b",
                        ratings: &ratings
                    }
                ]
            )?
            .disposition,
        Disposition::Failed
    );
    let old = Suite::complete()?;
    let stale = old
        .cases()
        .iter()
        .zip(&proposals)
        .map(|(c, p)| {
            Ok(c.input()
                .ok_or("input")?
                .presentation_id(&deployment, Candidate::Proposed(p), true))
        })
        .collect::<Result<Vec<_>>>()?;
    let stale = old
        .cases()
        .iter()
        .zip(&stale)
        .map(|(c, p)| Rating {
            id: c.id(),
            presentation: p,
            preference: Preference::First,
        })
        .collect::<Vec<_>>();
    assert!(
        suite
            .evaluate(
                &deployment,
                &results,
                [
                    Review {
                        reviewer: "a",
                        ratings: &stale
                    },
                    Review {
                        reviewer: "b",
                        ratings: &stale
                    }
                ]
            )
            .is_err()
    );
    let new = crate::explanation_qualification::runtime_protocol()?;
    let previous = crate::explanation_qualification::protocol()?;
    assert_ne!(new.0, previous.0);
    assert_ne!(new.2, previous.2);
    assert_eq!(new.1, previous.1);
    let probes = Suite::runtime_probe_inputs()?;
    assert_eq!(probes.len(), 4);
    for (_, bytes) in probes {
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        assert_eq!(value["kind"], INPUT_REVISION);
        assert!(value["reviewed_context"]["untrusted_context"]["document"].is_object());
    }
    assert_eq!(
        suite.identity(),
        "b28ce7cb889d64e8b20f397c0b2a56f66ab3630128c8f5c888d49f527346d994"
    );
    assert_eq!(
        new.0,
        "3f9aa8d7bbc26fea7e1dda7de7e9e7a40bca080c9e27595346393ffb6530d1b2"
    );
    Ok(())
}
