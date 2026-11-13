//! New frozen suite through the production implementation; historical v2 stays unchanged.
use super::*;
use crate::explanation_runtime::{
    INPUT_REVISION, ModelContext, RENDERER_REVISION, RuntimePreparation, ServingContext,
    VALIDATOR_REVISION,
};
const MANIFEST: &str = include_str!("../../../tests/fixtures/explanations/usefulness-v3.tsv");
const SUITE_PIN: &str = "b28ce7cb889d64e8b20f397c0b2a56f66ab3630128c8f5c888d49f527346d994";
fn document() -> DocumentExcerpt {
    DocumentExcerpt {
        source: "invented-reviewed-release".into(),
        scope: "evaluation".into(),
        available_at_ms: 100,
        text: "Invented reviewed context. It is not the source of the ranking facts.".into(),
    }
}
fn prepare(
    f: &recipe::Fixture,
    proposal: Option<&[u8]>,
    doc: Option<DocumentExcerpt>,
) -> Result<(String, Vec<u8>, Vec<u8>), EvaluationError> {
    let standard = Allowlist::from_verified(&f.record, &f.decision)
        .map_err(|_| INVALID)?
        .standard()
        .map_err(|_| INVALID)?
        .proposal()
        .canonical_bytes()
        .map_err(|_| INVALID)?;
    let has_context = doc.is_some();
    let doc = doc.unwrap_or_else(document);
    let bytes = doc.canonical_bytes().map_err(|_| INVALID)?;
    let model = Request {
        anchor: Allowlist::from_verified(&f.record, &f.decision)
            .map_err(|_| INVALID)?
            .standard()
            .map_err(|_| INVALID)?
            .anchor()
            .clone(),
        audience: "runtime-evaluation".into(),
        purpose: Purpose::ModelDisclosure,
        document: Some(doc.id().map_err(|_| INVALID)?),
    };
    let a = rights(&Request {
        document: None,
        ..model.clone()
    });
    let d = rights(&model);
    let context = has_context.then_some(ModelContext {
        bytes: &bytes,
        operation: &model,
        artifact: &a,
        document: &d,
        now_ms: 100,
    });
    let prepared = RuntimePreparation::from_verified(
        &f.record,
        &f.decision,
        &f.configuration,
        &f.input,
        context,
    )
    .map_err(|_| INVALID)?;
    let serving = Request {
        purpose: Purpose::ExplanationServing,
        ..model
    };
    let a = rights(&Request {
        document: None,
        ..serving.clone()
    });
    let d = rights(&serving);
    let observation = has_context.then_some(ServingContext {
        operation: &serving,
        artifact: &a,
        document: &d,
        now_ms: 100,
    });
    let rendered = prepared
        .validate(proposal.unwrap_or(&standard), observation)
        .map_err(|_| INVALID)?
        .rendered()
        .to_owned();
    let input = prepared.input().to_vec();
    drop(prepared);
    Ok((rendered, input, standard))
}
fn materialize(
    n: usize,
    proposal: Option<&[u8]>,
) -> Result<(recipe::Fixture, String, Vec<u8>, Vec<u8>), EvaluationError> {
    let f = recipe::build(n)?;
    let (text, input, standard) = prepare(&f, proposal, (n == 29).then(document))?;
    Ok((f, text, input, standard))
}
pub(crate) fn probe_inputs() -> Result<Vec<(String, Vec<u8>)>, EvaluationError> {
    let f = recipe::build(8)?;
    include_str!("../../../tests/fixtures/explanations/model-context-v1.tsv")
        .lines()
        .map(|line| {
            let (id, context) = line.split_once('\t').ok_or(INVALID)?;
            let doc = DocumentExcerpt {
                source: format!("invented-adversarial-{id}"),
                text: context.into(),
                ..document()
            };
            let (_, input, _) = prepare(&f, None, Some(doc))?;
            Ok((id.into(), input))
        })
        .collect()
}
pub(crate) fn render(n: usize, proposal: &[u8]) -> Result<String, crate::explanation::Error> {
    materialize(n, Some(proposal))
        .map(|(_, text, _, _)| text)
        .map_err(|_| crate::explanation::Error)
}
fn pin(
    id: &str,
    task: &str,
    f: &recipe::Fixture,
    baseline: &str,
    input: &[u8],
    standard: &[u8],
    variant: &str,
) -> Result<String, EvaluationError> {
    Ok(identity(&[
        id.as_bytes(),
        task.as_bytes(),
        INPUT_REVISION.as_bytes(),
        RENDERER_REVISION.as_bytes(),
        &edgeagent_contracts::canonical_json_bytes(&f.assertions).map_err(|_| INVALID)?,
        &f.configuration,
        input,
        &f.record.canonical_bytes().map_err(|_| INVALID)?,
        &f.decision.canonical_bytes().map_err(|_| INVALID)?,
        baseline.as_bytes(),
        standard,
        variant.as_bytes(),
    ]))
}
fn suite_pin(manifest: &str, old: &Suite) -> Result<String, EvaluationError> {
    let probes=probe_inputs()?.into_iter().map(|(id,input)|serde_json::json!({"id":id,"input":edgeagent_contracts::evidence::DailyEvidenceArtifact::hash(&input).as_str()})).collect::<Vec<_>>();
    let probes = edgeagent_contracts::canonical_json_bytes(&probes).map_err(|_| INVALID)?;
    Ok(identity(&[
        manifest.as_bytes(),
        old.identity().as_bytes(),
        RUBRIC,
        SAFETY,
        INPUT_REVISION.as_bytes(),
        RENDERER_REVISION.as_bytes(),
        VALIDATOR_REVISION.as_bytes(),
        safety::pin()?.as_bytes(),
        &probes,
    ]))
}
pub(crate) fn load() -> Result<Suite, EvaluationError> {
    let old = super::load()?;
    let mut cases = Vec::new();
    let mut pins_match = true;
    for (index, (line, previous)) in MANIFEST.lines().zip(old.cases()).enumerate() {
        let [id, recipe, frozen, scenario, task] = line.split('\t').collect::<Vec<_>>()[..] else {
            return Err(INVALID);
        };
        if (id, scenario, task) != (previous.id(), previous.scenario(), previous.task())
            || recipe != "runtime"
        {
            return Err(INVALID);
        }
        let (f, baseline, input, standard) = materialize(index + 1, None)?;
        semantic::check(index + 1, &f, &baseline)?;
        let mut plain = Proposal::decode(&standard).map_err(|_| INVALID)?;
        plain.claims[0].variant = Variant::PlainLanguage;
        let variant = render(index + 1, &plain.canonical_bytes().map_err(|_| INVALID)?)
            .map_err(|_| INVALID)?;
        semantic::check(index + 1, &f, &variant)?;
        let case_pin = pin(id, task, &f, &baseline, &input, &standard, &variant)?;
        pins_match &= case_pin == frozen;
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
                complete_case: None,
                runtime_case: Some(index + 1),
                prepared: Some(input),
            }),
        });
    }
    let identity = suite_pin(MANIFEST, &old)?;
    if cases.len() != 30 || MANIFEST.lines().count() != 30 || !pins_match || identity != SUITE_PIN {
        return Err(INVALID);
    }
    Ok(Suite {
        cases,
        identity,
        complete: true,
    })
}
