use edgeagent_contracts::explanation::{Proposal, Variant};
use edgeagent_research_application::explanation_evaluation::{
    Candidate, CaseResult, Disposition, Preference, Rating, Review, Suite,
};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
#[test]
fn all_thirty_tasks_have_pinned_semantically_checked_inputs_and_both_presentations() -> Result {
    let old = Suite::frozen()?;
    let suite = Suite::complete()?;
    assert_eq!(
        old.identity(),
        "2631d3bb0a693070a96592dd3b5c3d2ce3b39d1bcef9c5a54659c179cd23ca49"
    );
    assert_eq!(
        old.cases().iter().filter(|c| c.input().is_some()).count(),
        10
    );
    assert_ne!(suite.identity(), old.identity());
    assert_eq!(suite.cases().len(), 30);
    for (legacy, case) in old.cases().iter().zip(suite.cases()) {
        assert_eq!(
            (legacy.id(), legacy.scenario(), legacy.task()),
            (case.id(), case.scenario(), case.task())
        );
        let input = case.input().ok_or("uncovered reader task")?;
        let prepared: serde_json::Value =
            serde_json::from_slice(input.prepared_input().ok_or("missing prepared input")?)?;
        assert!(
            prepared["kind"] == "research.explanation-input.v4"
                || (case.id() == "U29" && prepared["kind"] == "research.explanation-input.v3")
        );
        if prepared["kind"] == "research.explanation-input.v4" {
            let edgeagent_contracts::research::ResearchRecord::Artifact(a) = input.artifact()
            else {
                return Err("artifact".into());
            };
            for key in ["prices", "opens"] {
                for rows in prepared["assertions"][key]
                    .as_array()
                    .ok_or("source rows")?
                {
                    assert_eq!(
                        rows.as_array().ok_or("selected rows")?.len(),
                        usize::from(a.request.lookback_sessions) + 1
                    );
                }
            }
        }
        assert_eq!(
            input.render_candidate(input.standard_proposal())?,
            input.baseline()
        );
        let mut plain = Proposal::decode(input.standard_proposal())?;
        plain.claims[0].variant = Variant::PlainLanguage;
        assert!(
            input
                .render_candidate(&plain.canonical_bytes()?)?
                .contains("parts per million")
        );
        plain.claims.pop();
        assert!(input.render_candidate(&plain.canonical_bytes()?).is_err());
    }
    Ok(())
}
#[test]
fn complete_census_uses_fresh_presentations_and_unchanged_scoring() -> Result {
    let suite = Suite::complete()?;
    let deployment = "a".repeat(64);
    let proposals = suite
        .cases()
        .iter()
        .map(|case| {
            let input = case.input().ok_or("complete case")?;
            let mut proposal = Proposal::decode(input.standard_proposal())?;
            proposal.claims[0].variant = Variant::PlainLanguage;
            Ok(proposal.canonical_bytes()?)
        })
        .collect::<Result<Vec<_>>>()?;
    let results: Vec<_> = suite
        .cases()
        .iter()
        .zip(&proposals)
        .map(|(c, proposal)| {
            let i = c.input().ok_or("complete case")?;
            Ok(CaseResult {
                id: c.id(),
                input: i.identity(),
                candidate_first: true,
                candidate: Candidate::Proposed(proposal),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let presentations: Vec<_> = suite
        .cases()
        .iter()
        .zip(&proposals)
        .map(|(c, proposal)| {
            let i = c.input().ok_or("complete case")?;
            Ok(i.presentation_id(&deployment, Candidate::Proposed(proposal), true))
        })
        .collect::<Result<Vec<_>>>()?;
    let ratings: Vec<_> = results
        .iter()
        .zip(&presentations)
        .enumerate()
        .map(|(n, (r, p))| Rating {
            id: r.id,
            presentation: p,
            preference: if n < 21 {
                Preference::First
            } else {
                Preference::Tie
            },
        })
        .collect();
    let reviews = [
        Review {
            reviewer: "a",
            ratings: &ratings,
        },
        Review {
            reviewer: "b",
            ratings: &ratings,
        },
    ];
    let report = suite.evaluate(&deployment, &results, reviews)?;
    assert_eq!(report.disposition, Disposition::Passed);
    assert_eq!(report.preferences, [21, 21]);
    assert!(report.unresolved.is_empty());
    assert!(report.safety.iter().all(|s| s.passed));
    assert_eq!(report.safety.len(), 47);
    assert!(
        suite
            .evaluate(&deployment, &results[..29], reviews)
            .is_err()
    );
    let mut duplicate = results.clone();
    duplicate[0] = duplicate[1];
    assert!(suite.evaluate(&deployment, &duplicate, reviews).is_err());
    let mut stale = ratings.clone();
    stale[0].presentation = "historical-v1-presentation";
    assert!(
        suite
            .evaluate(
                &deployment,
                &results,
                [
                    Review {
                        ratings: &stale,
                        ..reviews[0]
                    },
                    reviews[1]
                ]
            )
            .is_err()
    );
    let mut weak = ratings.clone();
    weak[0].preference = Preference::Tie;
    assert_eq!(
        suite
            .evaluate(
                &deployment,
                &results,
                [
                    Review {
                        ratings: &weak,
                        ..reviews[0]
                    },
                    reviews[1]
                ]
            )?
            .disposition,
        Disposition::Failed
    );
    Ok(())
}
