//! Bounded candidate selection on the invented, non-grading comparison fixtures.
use super::{INVALID, PolicyPreview, Variant, materialize, prepare, recipe};
use crate::explanation_evaluation::{DevelopmentLayout, EvaluationError, identity};
use edgeagent_contracts::explanation::{Claim, Proposal};
use serde_json::{Value, json};

const CASES: [(usize, &str, &str); 3] = [
    (
        201,
        "D01",
        "Find the leader, distinguish positive and negative scores, and identify the unit.",
    ),
    (
        202,
        "D02",
        "Find the excluded instrument, its observed turnover, and the applicable floor.",
    ),
    (
        203,
        "D03",
        "Find the source for the leader and distinguish event, observation, and availability times.",
    ),
];

struct DevelopmentCase {
    fixture: recipe::Fixture,
    standard: String,
    reader_first: String,
    proposal: Vec<u8>,
    binding: String,
}

impl DevelopmentCase {
    fn load(n: usize, id: &str, task: &str) -> Result<Self, EvaluationError> {
        let (fixture, standard, input, proposal) = materialize(n, None)?;
        let policy = PolicyPreview::from_verified(
            &fixture.record,
            &fixture.decision,
            &fixture.configuration,
            &fixture.input,
        )
        .map_err(|_| INVALID)?;
        let summary = policy.reader_first_summary().map_err(|_| INVALID)?;
        let reader_first = format!("{summary}\nFull validated presentation\n{standard}");
        let binding = identity(&[
            b"research.explanation-development-choice-input.v1",
            id.as_bytes(),
            task.as_bytes(),
            &input,
            standard.as_bytes(),
            reader_first.as_bytes(),
        ]);
        Ok(Self {
            fixture,
            standard,
            reader_first,
            proposal,
            binding,
        })
    }
}

pub(crate) fn model_case(
    case: &str,
) -> Result<(edgeagent_contracts::explanation::Anchor, Value), EvaluationError> {
    let &(n, id, task) = CASES
        .iter()
        .find(|row| row.1 == case)
        .ok_or(EvaluationError::Census)?;
    let prepared = DevelopmentCase::load(n, id, task)?;
    let anchor = crate::explanation::Allowlist::from_verified(
        &prepared.fixture.record,
        &prepared.fixture.decision,
    )
    .map_err(|_| INVALID)?
    .standard()
    .map_err(|_| INVALID)?
    .anchor()
    .clone();
    Ok((
        anchor,
        json!({
            "case":id, "input":prepared.binding, "task":task,
            "standard":prepared.standard, "reader_first":prepared.reader_first
        }),
    ))
}

pub(crate) fn render_choice(
    expected_case: &str,
    case: &str,
    input: &str,
    layout: DevelopmentLayout,
) -> Result<Value, EvaluationError> {
    if case != expected_case {
        return Err(EvaluationError::Census);
    }
    let &(n, id, task) = CASES
        .iter()
        .find(|row| row.1 == expected_case)
        .ok_or(EvaluationError::Census)?;
    let prepared = DevelopmentCase::load(n, id, task)?;
    if input != prepared.binding {
        return Err(EvaluationError::Census);
    }
    let (layout, presentation) = match layout {
        DevelopmentLayout::Standard => ("standard", prepared.standard),
        DevelopmentLayout::ReaderFirst => ("reader_first", prepared.reader_first),
    };
    Ok(
        json!({"kind":"research.explanation-development-selection.v1",
        "case":id,"input":input,"layout":layout,"presentation":presentation}),
    )
}

pub(crate) fn development_comparison() -> Result<Value, EvaluationError> {
    let mut rows = Vec::with_capacity(CASES.len());
    for (n, id, task) in CASES {
        let DevelopmentCase {
            fixture,
            standard,
            reader_first,
            proposal,
            binding,
        } = DevelopmentCase::load(n, id, task)?;
        let mut choice = Proposal::decode(&proposal).map_err(|_| INVALID)?;
        for selection in &mut choice.claims {
            if matches!(selection.claim, Claim::Ranked { .. }) {
                selection.variant = Variant::PlainLanguage;
            }
        }
        let candidate = choice.canonical_bytes().map_err(|_| INVALID)?;
        let (plain, _, _) = prepare(&fixture, Some(&candidate), None)?;

        let mut changed_rank_lines = 0;
        let mut changed_other_lines = 0;
        if standard.lines().count() != plain.lines().count() {
            return Err(INVALID);
        }
        for (before, after) in standard.lines().zip(plain.lines()) {
            if before == after {
                continue;
            }
            if before.starts_with("rank=") && after.starts_with("Rank ") {
                changed_rank_lines += 1;
            } else {
                changed_other_lines += 1;
            }
        }
        rows.push(
            json!({"id":id,"scope":fixture.decision.request.scope,"task":task,"choice_binding":binding,
            "standard":standard,"plain_language":plain,"reader_first":reader_first,"changed_rank_lines":changed_rank_lines,
            "changed_other_lines":changed_other_lines}),
        );
    }
    Ok(json!({"kind":"research.explanation-development-comparison.v1","cases":rows}))
}
