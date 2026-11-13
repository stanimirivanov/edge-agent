//! Invented owner-reviewed package for tests only; no real model was qualified.
use edgeagent_contracts::{
    explanation::{Proposal, Variant},
    explanation_profile::Profile,
    explanation_qualification::*,
};
use edgeagent_research_application::{
    explanation_evaluation::{Candidate, Suite},
    explanation_qualification::protocol,
};
pub fn approved() -> Result<Command, Box<dyn std::error::Error + Send + Sync>> {
    let suite = Suite::complete()?;
    let (preparation, validator, renderer) = protocol()?;
    let profile = Profile {
        deployment: edgeagent_contracts::explanation_profile::Deployment {
            runtime: "a".repeat(64),
            weights: "b".repeat(64),
        },
        settings: edgeagent_contracts::explanation_profile::Settings {
            seed: 1,
            temperature_milli: 0,
            max_output_tokens: 512,
            timeout_ms: 1000,
        },
        prompt: "c".repeat(64),
        validator,
        renderer,
        tools: edgeagent_contracts::explanation_profile::Tools::None,
    };
    let id = profile.id()?;
    let cases = suite
        .cases()
        .iter()
        .map(|case| {
            let input = case.input().ok_or("input")?;
            let mut proposal = Proposal::decode(input.standard_proposal())?;
            proposal.claims[0].variant = Variant::PlainLanguage;
            Ok(Case {
                id: case.id().into(),
                input: input.identity().into(),
                candidate_first: true,
                candidate: Observation::Generated(String::from_utf8(proposal.canonical_bytes()?)?),
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;
    let ratings = suite
        .cases()
        .iter()
        .zip(&cases)
        .map(|(case, result)| {
            let Observation::Generated(proposal) = &result.candidate else {
                return Err("proposal".into());
            };
            Ok(Rating {
                id: case.id().into(),
                presentation: case.input().ok_or("input")?.presentation_id(
                    &id,
                    Candidate::Proposed(proposal.as_bytes()),
                    true,
                ),
                preference: Preference::First,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;
    let safety = suite
        .cases()
        .iter()
        .find(|c| c.id() == "U08")
        .and_then(|c| c.input())
        .ok_or("safety input")?;
    let probes = edgeagent_research_application::explanation_qualification::context_inputs()?
        .into_iter()
        .map(|(id, input)| {
            Ok(Case {
                id,
                input,
                candidate_first: true,
                candidate: Observation::Generated(String::from_utf8(
                    safety.standard_proposal().into(),
                )?),
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;
    Ok(Command::Approve {
        scope: "scope".into(),
        revision: 1,
        attestation: "d".repeat(64),
        evidence: Box::new(Evidence {
            collection: Collection {
                profile,
                suite: suite.identity().into(),
                preparation,
                manifest: "e".repeat(64),
                helper: "f".repeat(64),
                provenance: Provenance::OwnedLocalRuntimeV1,
                cases,
                safety: probes,
            },
            reviews: [
                Review {
                    reviewer: "reviewer-a".into(),
                    ratings: ratings.clone(),
                },
                Review {
                    reviewer: "reviewer-b".into(),
                    ratings,
                },
            ],
        }),
    })
}
