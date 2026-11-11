use edgeagent_research_application::explanation_evaluation::{
    Candidate, CaseResult, Disposition, EvaluationError, Preference, Rating, Review, Suite,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn unresolved_tasks_keep_the_frozen_suite_incomplete() -> TestResult {
    let suite = Suite::frozen()?;
    assert_eq!(suite.cases().len(), 30);
    let results: Vec<_> = suite
        .cases()
        .iter()
        .filter_map(|case| {
            case.input().map(|input| CaseResult {
                id: case.id(),
                input: input.identity(),
                candidate_first: true,
                candidate: Candidate::Proposed(input.standard_proposal()),
            })
        })
        .collect();
    assert_eq!(results.len(), 10);
    let deployment = "a".repeat(64);
    let presentations: Vec<_> = suite
        .cases()
        .iter()
        .filter_map(|case| case.input())
        .zip(&results)
        .map(|(input, result)| {
            input.presentation_id(&deployment, result.candidate, result.candidate_first)
        })
        .collect();
    let ratings: Vec<_> = results
        .iter()
        .zip(&presentations)
        .map(|(result, presentation)| Rating {
            id: result.id,
            presentation,
            preference: Preference::First,
        })
        .collect();
    let reviews = [
        Review {
            reviewer: "reviewer-a",
            ratings: &ratings,
        },
        Review {
            reviewer: "reviewer-b",
            ratings: &ratings,
        },
    ];
    let report = suite.evaluate(&deployment, &results, reviews)?;
    assert_eq!(report.disposition, Disposition::Incomplete);
    assert!(report.unresolved.contains(&"U18"));
    assert!(report.unresolved.contains(&"U27"));
    assert_eq!(report.safety.len(), 17);
    assert!(report.safety.iter().all(|check| check.passed));
    assert_eq!(report.preferences, [10, 10]);
    assert_eq!(report.unresolved.len(), 20);
    let mut reversed = results.clone();
    reversed.reverse();
    let again = suite.evaluate(&deployment, &reversed, reviews)?;
    assert_eq!(
        again.pairs.iter().map(|pair| pair.id).collect::<Vec<_>>(),
        report.pairs.iter().map(|pair| pair.id).collect::<Vec<_>>()
    );

    let mut bad_ratings = ratings.clone();
    bad_ratings[0] = bad_ratings[1];
    assert!(matches!(
        suite.evaluate(
            &deployment,
            &results,
            [
                Review {
                    ratings: &bad_ratings,
                    ..reviews[0]
                },
                reviews[1]
            ]
        ),
        Err(EvaluationError::Census)
    ));
    assert!(matches!(
        suite.evaluate(&deployment, &results, [reviews[0]; 2]),
        Err(EvaluationError::Census)
    ));
    let mut failed = results.clone();
    failed[0].candidate = Candidate::Proposed(b"unsupported output");
    assert!(matches!(
        suite.evaluate(&deployment, &failed, reviews),
        Err(EvaluationError::Census)
    ));
    let rejected = suite.cases()[0]
        .input()
        .ok_or("missing U01")?
        .presentation_id(&deployment, failed[0].candidate, failed[0].candidate_first);
    let mut rejected_ratings = ratings.clone();
    rejected_ratings[0].presentation = &rejected;
    let report = suite.evaluate(
        &deployment,
        &failed,
        reviews.map(|review| Review {
            ratings: &rejected_ratings,
            ..review
        }),
    )?;
    assert_eq!(
        report.pairs[0].outcome,
        edgeagent_research_application::explanation_evaluation::Outcome::Rejected
    );
    assert_eq!(report.preferences, [9, 9]);

    assert!(matches!(
        suite.evaluate(&"a".repeat(64), &results[1..], reviews),
        Err(EvaluationError::Census)
    ));
    let mut duplicate = results.clone();
    duplicate.push(results[0]);
    assert!(matches!(
        suite.evaluate(&"a".repeat(64), &duplicate, reviews),
        Err(EvaluationError::Census)
    ));
    let mut altered = results.clone();
    altered[0].input = "different";
    assert!(matches!(
        suite.evaluate(&"a".repeat(64), &altered, reviews),
        Err(EvaluationError::Census)
    ));
    Ok(())
}

#[test]
fn frozen_manifest_preserves_all_documented_scenarios_and_tasks() -> TestResult {
    let suite = Suite::frozen()?;
    let protocol = include_str!("../../../docs/development/explanations.md");
    let expected: Vec<_> = protocol
        .lines()
        .filter(|line| line.starts_with("| U"))
        .collect();
    let actual: Vec<_> = suite
        .cases()
        .iter()
        .map(|case| format!("| {} | {} | {} |", case.id(), case.scenario(), case.task()))
        .collect();
    assert_eq!(actual, expected);
    Ok(())
}
