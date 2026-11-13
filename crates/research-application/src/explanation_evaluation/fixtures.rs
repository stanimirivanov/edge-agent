use super::{Case, EvaluationError, Input, PAIRS, Suite, identity, safety};
use crate::explanation::Allowlist;
use edgeagent_contracts::{
    research::{EvidenceRef, RankedCandidate, ResearchRecord},
    research_decision::{Assessment, DecisionProvenance, Reason, Status},
};

const MANIFEST: &str = include_str!("../../tests/fixtures/explanations/usefulness-v1.tsv");
const ARTIFACT: &[u8] = include_bytes!("../../../contracts/fixtures/research/v1/artifact.json");
const DECISION: &[u8] = include_bytes!("../../../contracts/fixtures/research/decision-v1.json");
const RUBRIC: &[u8] = b"v1: thirty pairs; two independent reviewers; minimum 21 each; ties and fallback score zero; no safety failures";
const RENDERER: &[u8] = b"research-explanation-trace.v1";
const INVALID: EvaluationError = EvaluationError::Fixture;

pub(super) fn load() -> Result<Suite, EvaluationError> {
    let mut cases = Vec::with_capacity(PAIRS);
    for (index, line) in MANIFEST.lines().enumerate() {
        let [id, recipe, pin, scenario, task] = line.split('\t').collect::<Vec<_>>()[..] else {
            return Err(INVALID);
        };
        if id != format!("U{:02}", index + 1) || scenario.is_empty() || task.is_empty() {
            return Err(INVALID);
        }
        let input = if recipe == "-" && pin == "-" {
            None
        } else {
            let input = build(id, recipe, task)?;
            if input.identity != pin {
                return Err(INVALID);
            }
            Some(input)
        };
        cases.push(Case {
            id,
            scenario,
            task,
            input,
        });
    }
    if cases.len() != PAIRS {
        return Err(INVALID);
    }
    let identity = identity(&[
        MANIFEST.as_bytes(),
        RUBRIC,
        RENDERER,
        safety::pin()?.as_bytes(),
    ]);
    if identity != "2631d3bb0a693070a96592dd3b5c3d2ce3b39d1bcef9c5a54659c179cd23ca49" {
        return Err(INVALID);
    }
    Ok(Suite {
        cases,
        identity,
        complete: false,
    })
}

pub(super) fn build(id: &str, recipe: &str, task: &str) -> Result<Input, EvaluationError> {
    use Reason::{GapExceeded, LiquidityBelowMinimum, VolatilityExceeded};
    type FixtureShape<'a> = (&'a [(&'a str, i64)], &'a [Reason]);
    let (ranks, reasons): FixtureShape<'_> = match recipe {
        "one" => (&[("AAA", 10_000)], &[]),
        "two" => (&[("AAA", 20_000), ("BBB", 10_000)], &[]),
        "narrow" => (&[("AAA", 20_001), ("BBB", 20_000), ("CCC", 5_000)], &[]),
        "negative" => (&[("AAA", -10_000), ("BBB", -20_000)], &[]),
        "zero" => (&[("AAA", 10_000), ("BBB", 0), ("CCC", -10_000)], &[]),
        "similar" => (&[("AAA1", 20_000), ("AAA2", 10_000)], &[]),
        "liquidity" => (&[("AAA", 10_000)], &[LiquidityBelowMinimum]),
        "limits" => (&[("AAA", 10_000)], &[VolatilityExceeded, GapExceeded]),
        "safety" => (
            &[("AAA", 20_000), ("BBB", 10_000)],
            &[LiquidityBelowMinimum],
        ),
        _ => return Err(INVALID),
    };
    let mut record = ResearchRecord::decode(ARTIFACT).map_err(|_| INVALID)?;
    let ResearchRecord::Artifact(artifact) = &mut record else {
        return Err(INVALID);
    };
    artifact.request.request_id = format!("explanation-{id}");
    artifact.request.instruments = ranks
        .iter()
        .map(|(instrument, _)| (*instrument).into())
        .collect();
    let eligible = artifact.request.instruments.clone();
    if !reasons.is_empty() {
        artifact.request.instruments.push("ZZZ".into());
    }
    artifact.ranking = ranks
        .iter()
        .map(|(instrument, score_ppm)| RankedCandidate {
            instrument: (*instrument).into(),
            score_ppm: *score_ppm,
        })
        .collect();
    artifact.evidence = artifact
        .request
        .instruments
        .iter()
        .zip(['1', '2', '3', '4'])
        .map(|(instrument, digit)| EvidenceRef {
            instrument: instrument.clone(),
            available_at_ms: 90,
            digest: digit.to_string().repeat(64),
        })
        .collect();
    artifact.explanation = "Invented explanation evaluation fixture.".into();
    let mut decision = DecisionProvenance::decode(DECISION).map_err(|_| INVALID)?;
    decision.request = artifact.request.clone();
    decision.assessment = Assessment::Evaluated {
        status: if reasons.is_empty() {
            Status::Approved
        } else {
            Status::Narrowed
        },
        eligible,
        reasons: reasons.to_vec(),
    };
    decision.outcome = record.digest().map_err(|_| INVALID)?;
    let allowlist = Allowlist::from_verified(&record, &decision).map_err(|_| INVALID)?;
    let validated = allowlist.standard().map_err(|_| INVALID)?;
    let baseline = validated.rendered().to_owned();
    let standard = validated
        .proposal()
        .canonical_bytes()
        .map_err(|_| INVALID)?;
    let identity = identity(&[
        id.as_bytes(),
        task.as_bytes(),
        RENDERER,
        &record.canonical_bytes().map_err(|_| INVALID)?,
        &decision.canonical_bytes().map_err(|_| INVALID)?,
        baseline.as_bytes(),
        &standard,
    ]);
    Ok(Input {
        identity,
        record,
        decision,
        baseline,
        standard,
        complete_case: None,
        runtime_case: None,
        prepared: None,
    })
}
