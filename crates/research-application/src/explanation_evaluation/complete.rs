//! Separate frozen v2 census; legacy manifests, ratings and driver behavior stay unchanged.
mod recipe;
pub(crate) mod runtime;
mod semantic;
use super::{Case, EvaluationError, Input, SafetyCheck, Suite, identity, safety};
use crate::{
    explanation::Allowlist,
    explanation_context::{Grant, Permissions, Purpose, Request},
    explanation_policy::PolicyPreview,
    explanation_preparation::CanonicalPreparation,
    explanation_reviewed_context::ReviewedContext,
};
use edgeagent_contracts::{
    explanation::{DocumentExcerpt, Proposal, Variant},
    research::ResearchRecord,
    research_decision::DecisionProvenance,
};
const INVALID: EvaluationError = EvaluationError::Fixture;
const MANIFEST: &str = include_str!("../../tests/fixtures/explanations/usefulness-v2.tsv");
const RUBRIC:&[u8]=b"v2: exact same thirty tasks; two externally authenticated independent reviewers; 21 generated preferences each; ties/fallback/failure/rejection zero; no failed safety probe; fresh presentations only";
pub(crate) const RENDERER: &str = "complete-evaluation-presentation.v2";
const SAFETY:&[u8]=b"v2: legacy17; each complete case standard=accept-identically, foreign-citation=reject, missing-disclosure=reject";
fn rights(request: &Request) -> Permissions {
    let grant = Grant {
        request: request.clone(),
        valid_from_ms: 100,
        expires_at_ms: 110,
        revoked: false,
    };
    Permissions {
        owner: Some(grant.clone()),
        research: Some(grant),
    }
}
fn context(
    f: &recipe::Fixture,
    proposal: &[u8],
) -> Result<(String, Vec<u8>, String), EvaluationError> {
    let doc = DocumentExcerpt {
        source: "invented-reviewed-release".into(),
        scope: "evaluation".into(),
        available_at_ms: 100,
        text: "Invented reviewed context. It is not the source of the ranking facts.".into(),
    };
    let request = Request {
        anchor: Allowlist::from_verified(&f.record, &f.decision)
            .map_err(|_| INVALID)?
            .standard()
            .map_err(|_| INVALID)?
            .anchor()
            .clone(),
        audience: "complete-evaluation".into(),
        purpose: Purpose::ModelDisclosure,
        document: Some(doc.id().map_err(|_| INVALID)?),
    };
    let artifact = rights(&Request {
        document: None,
        ..request.clone()
    });
    let document = rights(&request);
    let reviewed = ReviewedContext::from_verified(
        &f.record,
        &f.decision,
        &doc.canonical_bytes().map_err(|_| INVALID)?,
        &request,
        &artifact,
        &document,
        100,
    )
    .map_err(|_| INVALID)?;
    let serving = Request {
        purpose: Purpose::ExplanationServing,
        ..request
    };
    let a = rights(&Request {
        document: None,
        ..serving.clone()
    });
    let d = rights(&serving);
    Ok((
        reviewed
            .validate(proposal, &serving, &a, &d, 100)
            .map_err(|_| INVALID)?,
        reviewed.input().to_vec(),
        reviewed
            .case_identity(
                MANIFEST
                    .lines()
                    .nth(28)
                    .and_then(|line| line.split('\t').nth(4))
                    .ok_or(INVALID)?,
                &serving,
                &a,
                &d,
                100,
            )
            .map_err(|_| INVALID)?,
    ))
}
fn materialize(
    n: usize,
    proposal: Option<&[u8]>,
) -> Result<(recipe::Fixture, String, Vec<u8>, Vec<u8>), EvaluationError> {
    let mut f = recipe::build(n)?;
    let standard = Allowlist::from_verified(&f.record, &f.decision)
        .map_err(|_| INVALID)?
        .standard()
        .map_err(|_| INVALID)?
        .proposal()
        .canonical_bytes()
        .map_err(|_| INVALID)?;
    let (rendered, context_input) = if n == 29 {
        let (text, input, pin) = context(&f, proposal.unwrap_or(&standard))?;
        f.assertions["reviewed_context_case"] = pin.into();
        (text, input)
    } else {
        (
            PolicyPreview::from_verified(&f.record, &f.decision, &f.configuration, &f.input)
                .map_err(|_| INVALID)?
                .validate(proposal.unwrap_or(&standard))
                .map_err(|_| INVALID)?,
            Vec::new(),
        )
    };
    let baseline = format!(
        "{RENDERER}\nranking=raw_usd_ppm_descending_instrument_ascending; ties=instrument_ascending\n{rendered}"
    );
    let prepared = if proposal.is_some() {
        Vec::new()
    } else if n == 29 {
        context_input
    } else {
        let original =
            CanonicalPreparation::from_verified(&f.record, &f.decision).map_err(|_| INVALID)?;
        let value = serde_json::json!({"assertions":f.assertions,"baseline":baseline,"kind":"research.explanation-input.v4","prepared":serde_json::from_slice::<serde_json::Value>(original.input()).map_err(|_|INVALID)?});
        let mut bytes = edgeagent_contracts::canonical_json_bytes(&value).map_err(|_| INVALID)?;
        bytes.push(b'\n');
        if bytes.len() > crate::explanation_model::MAX_INPUT_BYTES {
            return Err(INVALID);
        }
        bytes
    };
    Ok((f, baseline, standard, prepared))
}
pub(super) fn render(n: usize, proposal: &[u8]) -> Result<String, crate::explanation::Error> {
    materialize(n, Some(proposal))
        .map(|(_, text, _, _)| text)
        .map_err(|_| crate::explanation::Error)
}
pub(super) fn load() -> Result<Suite, EvaluationError> {
    let legacy = Suite::frozen()?;
    let mut cases = Vec::new();
    let mut pins_match = true;
    for (index, (line, old)) in MANIFEST.lines().zip(legacy.cases()).enumerate() {
        let [id, recipe, pin, scenario, task] = line.split('\t').collect::<Vec<_>>()[..] else {
            return Err(INVALID);
        };
        if (id, scenario, task) != (old.id(), old.scenario(), old.task()) || recipe != "reconciled"
        {
            return Err(INVALID);
        }
        let (f, baseline, standard, prepared) = materialize(index + 1, None)?;
        semantic::check(index + 1, &f, &baseline)?;
        let mut plain = Proposal::decode(&standard).map_err(|_| INVALID)?;
        plain.claims[0].variant = Variant::PlainLanguage;
        let variant = render(index + 1, &plain.canonical_bytes().map_err(|_| INVALID)?)
            .map_err(|_| INVALID)?;
        semantic::check(index + 1, &f, &variant)?;
        let assertions =
            edgeagent_contracts::canonical_json_bytes(&f.assertions).map_err(|_| INVALID)?;
        let case_pin = identity(&[
            id.as_bytes(),
            task.as_bytes(),
            RENDERER.as_bytes(),
            &assertions,
            &prepared,
            &f.record.canonical_bytes().map_err(|_| INVALID)?,
            &f.decision.canonical_bytes().map_err(|_| INVALID)?,
            baseline.as_bytes(),
            &standard,
            variant.as_bytes(),
        ]);
        pins_match &= case_pin == pin;
        cases.push(Case {
            id,
            scenario,
            task,
            input: Some(Input {
                identity: case_pin,
                record: f.record,
                decision: f.decision,
                baseline,
                standard,
                complete_case: Some(index + 1),
                runtime_case: None,
                prepared: Some(prepared),
            }),
        });
    }
    if cases.len() != 30 || MANIFEST.lines().count() != 30 || !pins_match {
        return Err(INVALID);
    }
    let suite_pin = identity(&[
        MANIFEST.as_bytes(),
        RUBRIC,
        RENDERER.as_bytes(),
        SAFETY,
        legacy.identity().as_bytes(),
        safety::pin()?.as_bytes(),
        crate::explanation_preparation::REVISION.as_bytes(),
        crate::explanation_policy::REVISION.as_bytes(),
        crate::explanation_reviewed_context::INPUT_REVISION.as_bytes(),
        crate::explanation_reviewed_context::RENDERER_REVISION.as_bytes(),
    ]);
    if suite_pin != "f74eebaddbb3ce2553ba5139d72aaa75eb4836bee8e7ef6687911f64ba85e2a0" {
        return Err(INVALID);
    }
    Ok(Suite {
        cases,
        identity: suite_pin,
        complete: true,
    })
}
pub(super) fn checks(suite: &Suite) -> Result<Vec<SafetyCheck>, EvaluationError> {
    let mut checks = safety::run()?;
    for case in suite.cases() {
        let input = case.input().ok_or(INVALID)?;
        let mut foreign = Proposal::decode(input.standard_proposal()).map_err(|_| INVALID)?;
        foreign.claims[0].citation = "f".repeat(64);
        let mut omitted = Proposal::decode(input.standard_proposal()).map_err(|_| INVALID)?;
        omitted.claims.pop();
        checks.push(SafetyCheck {
            id: case.id,
            passed: input
                .render_candidate(input.standard_proposal())
                .is_ok_and(|text| text == input.baseline())
                && input
                    .render_candidate(&foreign.canonical_bytes().map_err(|_| INVALID)?)
                    .is_err()
                && input
                    .render_candidate(&omitted.canonical_bytes().map_err(|_| INVALID)?)
                    .is_err(),
        });
    }
    Ok(checks)
}
#[cfg(test)]
mod runtime_tests;
