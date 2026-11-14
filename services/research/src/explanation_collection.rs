//! Private runtime-parity qualification collection; never installs approval.
use crate::{
    explanation_deployment::Verifier,
    explanation_model::HttpModel,
    explanation_owned::{Manifest, digest},
    runtime::Result,
};
use edgeagent_contracts::{
    explanation::Binding,
    explanation_qualification::{Case, Collection, Observation, Provenance},
};
use edgeagent_research_application::{
    explanation::Allowlist,
    explanation_evaluation::{Candidate, Suite},
    explanation_model::{ExplanationModel, Request},
};
use serde_json::json;
use std::{
    io::{Read as _, Write as _},
    net::{Ipv4Addr, SocketAddrV4},
};

pub(crate) async fn collect() -> Result<()> {
    let (manifest, pin) = Manifest::environment()?;
    let profile = manifest.profile()?;
    let prompt = manifest.verify(&profile)?;
    let suite = Suite::runtime()?;
    let (preparation, validator, renderer) =
        edgeagent_research_application::explanation_qualification::runtime_protocol()?;
    if profile.validator != validator || profile.renderer != renderer {
        return Err("runtime protocol required".into());
    }
    let observation = manifest.observation()?;
    let port = observation.port;
    let verifier = Verifier::new(observation, profile.deployment.clone())?;
    let model = HttpModel::new(
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, port),
        verifier.clone(),
        prompt,
    )?;
    let probes = Suite::runtime_probe_inputs()?;
    let safety_input = suite
        .cases()
        .iter()
        .find(|c| c.id() == "U08")
        .and_then(|c| c.input())
        .ok_or("probe input")?;
    let mut cases = Vec::new();
    let mut safety = Vec::new();
    let mut pairs = Vec::new();
    let mut random = [0; 34];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    for index in 0..34 {
        let (id, input, prepared, identity) = if index < 30 {
            let case = &suite.cases()[index];
            let input = case.input().ok_or("missing case")?;
            (
                case.id(),
                input,
                input.prepared_input().ok_or("missing prepared input")?,
                input.identity().to_owned(),
            )
        } else {
            let (id, bytes) = &probes[index - 30];
            (id.as_str(), safety_input, bytes.as_slice(), digest(bytes))
        };
        let binding = Binding {
            anchor: Allowlist::from_verified(input.artifact(), input.decision())?
                .standard()?
                .anchor()
                .clone(),
            profile: profile.id()?,
            evidence: digest(prepared),
        };
        let request = Request::new(&profile, &binding, prepared)?;
        let reply = model.propose(&request).await;
        verifier.verify()?;
        let (candidate, text) = match reply {
            Ok(reply) => match input.render_candidate(reply.proposal()) {
                Ok(text) => (
                    Observation::Generated(std::str::from_utf8(reply.proposal())?.into()),
                    text,
                ),
                Err(_) => (Observation::Fallback, input.baseline().into()),
            },
            Err(_) => (Observation::Failed, input.baseline().into()),
        };
        let first = random[index] & 1 == 1;
        if index < 30 {
            let proposed = match &candidate {
                Observation::Generated(p) => Candidate::Proposed(p.as_bytes()),
                Observation::Fallback => Candidate::Fallback,
                Observation::Failed => Candidate::Failed,
            };
            let presentation = input.presentation_id(&profile.id()?, proposed, first);
            let (a, b) = if first {
                (text, input.baseline().into())
            } else {
                (input.baseline().into(), text)
            };
            pairs.push(json!({"id":id,"presentation":presentation,"first":a,"second":b}));
        }
        let case = Case {
            id: id.into(),
            input: identity,
            candidate_first: first,
            candidate,
        };
        if index < 30 {
            cases.push(case);
        } else {
            safety.push(case);
        }
    }
    let collection = Collection {
        profile,
        suite: suite.identity().into(),
        preparation,
        manifest: pin,
        helper: manifest.helper.sha256.clone(),
        provenance: Provenance::OwnedLocalRuntimeV1,
        cases,
        safety,
    };
    let output = json!({"kind":"research.explanation-runtime.collection.v2","collection":collection,"pairs":pairs,"permission_granted":false});
    let mut bytes = edgeagent_contracts::canonical_json_bytes(&output)?;
    bytes.push(b'\n');
    if bytes.len() > edgeagent_contracts::explanation_qualification::MAX_COMMAND_BYTES {
        return Err("collection exceeds bound".into());
    }
    std::io::stdout().write_all(&bytes)?;
    Ok(())
}
