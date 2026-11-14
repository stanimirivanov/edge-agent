//! Private runtime-parity qualification collection; never installs approval.
use crate::{
    explanation_deployment::Verifier,
    explanation_model::{HttpModel, ModelStage},
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
    explanation_model::{ExplanationModel, Failure, Request},
};
use serde_json::json;
use std::{
    io::{Read as _, Write as _},
    net::{Ipv4Addr, SocketAddrV4},
    time::Instant,
};

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum AttemptStatus {
    InvalidRequest,
    Unavailable,
    Timeout,
    Refused,
    BudgetExhausted,
    DeploymentMismatch,
    InvalidResponse,
    InvalidProposal,
    Generated,
}
impl From<Failure> for AttemptStatus {
    fn from(value: Failure) -> Self {
        match value {
            Failure::InvalidRequest => Self::InvalidRequest,
            Failure::Unavailable => Self::Unavailable,
            Failure::Timeout => Self::Timeout,
            Failure::Refused => Self::Refused,
            Failure::BudgetExhausted => Self::BudgetExhausted,
            Failure::DeploymentMismatch => Self::DeploymentMismatch,
            Failure::InvalidResponse => Self::InvalidResponse,
        }
    }
}
struct DiagnosticReport(std::fs::File);
impl DiagnosticReport {
    fn open(
        path: &std::path::Path,
        source: &str,
        manifest: &str,
        profile: &str,
        suite: &str,
        preparation: &str,
    ) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt as _;
        if source.len() != 64
            || !source.bytes().all(|b| b.is_ascii_hexdigit())
            || source.bytes().all(|b| b == b'0')
        {
            return Err("invalid diagnostic source".into());
        }
        let mut report = Self(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)?,
        );
        report.append(json!({"kind":"research.explanation-runtime.diagnostics.v1","source_collection":source,
            "manifest":manifest,"profile":profile,"suite":suite,"preparation":preparation,"permission_granted":false}))?;
        std::fs::File::open(path.parent().ok_or("diagnostic parent required")?)?.sync_all()?;
        Ok(report)
    }
    fn append(&mut self, value: serde_json::Value) -> Result<()> {
        self.0
            .write_all(&edgeagent_contracts::canonical_json_bytes(&value)?)?;
        self.0.write_all(b"\n")?;
        self.0.sync_all()?;
        Ok(())
    }
    fn started(&mut self, id: &str, input: &str) -> Result<()> {
        self.append(json!({"kind":"attempt_started","id":id,"input":input}))
    }
    fn finished(
        &mut self,
        id: &str,
        input: &str,
        status: AttemptStatus,
        stage: ModelStage,
        elapsed_ms: u64,
    ) -> Result<()> {
        self.append(json!({"kind":"attempt_finished","id":id,"input":input,"status":status,"stage":stage,"elapsed_ms":elapsed_ms}))
    }
    fn complete(&mut self, attempts: usize) -> Result<()> {
        self.append(
            json!({"kind":"diagnostic_completed","attempts":attempts,"permission_granted":false}),
        )
    }
}

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
    let mut diagnostic = match (
        std::env::var_os("EDGEAGENT_EXPLANATION_DIAGNOSTIC_REPORT"),
        std::env::var("EDGEAGENT_EXPLANATION_DIAGNOSTIC_SOURCE"),
    ) {
        (None, Err(std::env::VarError::NotPresent)) => None,
        (Some(path), Ok(source)) => Some(DiagnosticReport::open(
            std::path::Path::new(&path),
            &source,
            &pin,
            &profile.id()?,
            suite.identity(),
            &preparation,
        )?),
        _ => return Err("incomplete diagnostic configuration".into()),
    };
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
        if let Some(report) = &mut diagnostic {
            report.started(id, &identity)?;
        }
        let started = Instant::now();
        let reply = model.propose(&request).await;
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        verifier.verify()?;
        let (candidate, text, status) = match reply {
            Ok(reply) => match input.render_candidate(reply.proposal()) {
                Ok(text) => (
                    Observation::Generated(std::str::from_utf8(reply.proposal())?.into()),
                    text,
                    AttemptStatus::Generated,
                ),
                Err(_) => (
                    Observation::Fallback,
                    input.baseline().into(),
                    AttemptStatus::InvalidProposal,
                ),
            },
            Err(failure) => (Observation::Failed, input.baseline().into(), failure.into()),
        };
        if let Some(report) = &mut diagnostic {
            report.finished(id, &identity, status, model.diagnostic_stage(), elapsed)?;
        }
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
    if let Some(mut report) = diagnostic {
        report.complete(34)?;
        std::io::stdout().write_all(b"{\"kind\":\"research.explanation-runtime.diagnostics-complete.v1\",\"permission_granted\":false}\n")?;
        return Ok(());
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

#[cfg(test)]
mod tests {
    use super::*;
    use edgeagent_research_application::explanation_model::Failure;

    #[test]
    fn diagnostic_statuses_are_closed_payload_free_and_exhaustive() -> Result<()> {
        for (failure, expected) in [
            (Failure::InvalidRequest, "invalid_request"),
            (Failure::Unavailable, "unavailable"),
            (Failure::Timeout, "timeout"),
            (Failure::Refused, "refused"),
            (Failure::BudgetExhausted, "budget_exhausted"),
            (Failure::DeploymentMismatch, "deployment_mismatch"),
            (Failure::InvalidResponse, "invalid_response"),
        ] {
            assert_eq!(
                serde_json::to_value(AttemptStatus::from(failure))?,
                expected
            );
        }
        assert_eq!(
            serde_json::to_value(AttemptStatus::InvalidProposal)?,
            "invalid_proposal"
        );
        assert_eq!(serde_json::to_value(AttemptStatus::Generated)?, "generated");
        Ok(())
    }

    #[test]
    fn diagnostic_records_are_private_durable_exclusive_and_not_qualification() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        let path =
            std::env::temp_dir().join(format!("explanation-diagnostic-{}", std::process::id()));
        let pin = "a".repeat(64);
        let mut report = DiagnosticReport::open(&path, &pin, &pin, &pin, &pin, &pin)?;
        assert_eq!(
            std::fs::metadata(&path)?.permissions().mode() & 0o777,
            0o600
        );
        assert!(DiagnosticReport::open(&path, &pin, &pin, &pin, &pin, &pin).is_err());
        report.started("U01", &pin)?;
        report.finished(
            "U01",
            &pin,
            AttemptStatus::Timeout,
            ModelStage::WaitingResponse,
            30001,
        )?;
        // The completed attempt is readable while the writer is still alive.
        let raw = std::fs::read_to_string(&path)?;
        let rows = raw
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0]["kind"],
            "research.explanation-runtime.diagnostics.v1"
        );
        assert_eq!(rows[0]["permission_granted"], false);
        assert!(rows[0].get("collection").is_none());
        assert_eq!(rows[2]["status"], "timeout");
        assert_eq!(rows[2]["elapsed_ms"], 30001);
        assert!(rows[2].get("proposal").is_none());
        report.complete(1)?;
        drop(report);
        assert_eq!(std::fs::read_to_string(&path)?.lines().count(), 4);
        std::fs::remove_file(path)?;
        Ok(())
    }
}
