//! Rescore complete qualification evidence; only authenticated owner review can grant approval.
use crate::explanation_evaluation::{self as eval, Candidate, CaseResult, Disposition, Suite};
use edgeagent_contracts::{
    canonical_json_bytes,
    evidence::DailyEvidenceArtifact as Hash,
    explanation_qualification::{Command, Error, Observation, Preference},
};
const CONTEXTS: &str = include_str!("../tests/fixtures/explanations/model-context-v1.tsv");
fn digest(value: serde_json::Value) -> Result<String, Error> {
    let mut bytes = canonical_json_bytes(&value).map_err(|_| Error)?;
    bytes.push(b'\n');
    Ok(Hash::hash(&bytes).as_str().into())
}
fn contexts(suite: &Suite) -> Result<Vec<(String, String)>, Error> {
    let input = suite
        .cases()
        .iter()
        .find(|c| c.id() == "U08")
        .and_then(|c| c.input())
        .ok_or(Error)?;
    CONTEXTS.lines().map(|line| {
        let (id,context)=line.split_once('\t').ok_or(Error)?;
        let prepared:serde_json::Value=serde_json::from_slice(input.prepared_input().ok_or(Error)?).map_err(|_|Error)?;
        Ok((id.into(),digest(serde_json::json!({"kind":"research.explanation-qualification.context.v1","prepared":prepared,"untrusted_context":context}))?))
    }).collect()
}
/// Frozen adversarial-context identities for a future owned collection producer.
/// # Errors
/// Refuses unavailable or drifted complete inputs.
pub fn context_inputs() -> Result<Vec<(String, String)>, Error> {
    contexts(&Suite::complete().map_err(|_| Error)?)
}
/// Compiled complete preparation census, validator and renderer identities.
/// # Errors
/// Refuses drift or unavailable frozen inputs.
pub fn protocol() -> Result<(String, String, String), Error> {
    let suite = Suite::complete().map_err(|_| Error)?;
    let cases=suite.cases().iter().map(|c| {
        let input=c.input().ok_or(Error)?;
        Ok(serde_json::json!({"id":c.id(),"case":input.identity(),"input":Hash::hash(input.prepared_input().ok_or(Error)?).as_str()}))
    }).collect::<Result<Vec<_>,Error>>()?;
    Ok((
        digest(
            serde_json::json!({"kind":"research.explanation-qualification.preparation.v1","suite":suite.identity(),"cases":cases,"contexts":contexts(&suite)?}),
        )?,
        Hash::hash(b"research-explanation-allowlist-v1")
            .as_str()
            .into(),
        Hash::hash(crate::explanation_evaluation::complete::RENDERER.as_bytes())
            .as_str()
            .into(),
    ))
}
/// Sealed, independently rescored review. Cannot be constructed from a passed flag.
pub struct VerifiedReview {
    command: Command,
    bytes: Vec<u8>,
    collection: String,
    assessment: String,
}
impl VerifiedReview {
    /// Validate exact bounded bytes and independently rescore every approval.
    /// Owner attestation of provenance/independence is enforced by persistence.
    /// # Errors
    /// Rejects incomplete, failed, mismatched, malformed or unsupported evidence.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let command = Command::decode(bytes)?;
        let mut collection = String::new();
        let mut assessment = String::new();
        if let Command::Approve { evidence, .. } | Command::ApproveRuntime { evidence, .. } =
            &command
        {
            let c = &evidence.collection;
            let runtime = matches!(command, Command::ApproveRuntime { .. });
            let suite = if runtime {
                Suite::runtime()
            } else {
                Suite::complete()
            }
            .map_err(|_| Error)?;
            let (preparation, validator, renderer) = if runtime {
                runtime_protocol()?
            } else {
                protocol()?
            };
            if c.suite != suite.identity()
                || c.preparation != preparation
                || c.profile.validator != validator
                || c.profile.renderer != renderer
            {
                return Err(Error);
            }
            let deployment = c.profile.id().map_err(|_| Error)?;
            let expected = if runtime {
                Suite::runtime_probe_inputs()
                    .map_err(|_| Error)?
                    .into_iter()
                    .map(|(id, bytes)| (id, Hash::hash(&bytes).as_str().into()))
                    .collect()
            } else {
                contexts(&suite)?
            };
            if c.safety.len() != expected.len() {
                return Err(Error);
            }
            let safety_input = suite
                .cases()
                .iter()
                .find(|case| case.id() == "U08")
                .and_then(|case| case.input())
                .ok_or(Error)?;
            for (case, (id, input)) in c.safety.iter().zip(expected) {
                if case.id != id || case.input != input {
                    return Err(Error);
                }
                let Observation::Generated(proposal) = &case.candidate else {
                    return Err(Error);
                };
                safety_input
                    .render_candidate(proposal.as_bytes())
                    .map_err(|_| Error)?;
            }
            let mut results = Vec::new();
            for case in &c.cases {
                let input = suite
                    .cases()
                    .iter()
                    .find(|expected| expected.id() == case.id)
                    .and_then(|c| c.input())
                    .ok_or(Error)?;
                let candidate = match &case.candidate {
                    Observation::Generated(proposal) => {
                        input
                            .render_candidate(proposal.as_bytes())
                            .map_err(|_| Error)?;
                        Candidate::Proposed(proposal.as_bytes())
                    }
                    Observation::Fallback => Candidate::Fallback,
                    Observation::Failed => Candidate::Failed,
                };
                results.push(CaseResult {
                    id: &case.id,
                    input: &case.input,
                    candidate_first: case.candidate_first,
                    candidate,
                });
            }
            let ratings = evidence
                .reviews
                .iter()
                .map(|r| {
                    r.ratings
                        .iter()
                        .map(|rating| eval::Rating {
                            id: &rating.id,
                            presentation: &rating.presentation,
                            preference: match rating.preference {
                                Preference::First => eval::Preference::First,
                                Preference::Second => eval::Preference::Second,
                                Preference::Tie => eval::Preference::Tie,
                            },
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let report = suite
                .evaluate(
                    &deployment,
                    &results,
                    [
                        eval::Review {
                            reviewer: &evidence.reviews[0].reviewer,
                            ratings: &ratings[0],
                        },
                        eval::Review {
                            reviewer: &evidence.reviews[1].reviewer,
                            ratings: &ratings[1],
                        },
                    ],
                )
                .map_err(|_| Error)?;
            if report.disposition != Disposition::Passed
                || !report.unresolved.is_empty()
                || report.safety.iter().any(|s| !s.passed)
            {
                return Err(Error);
            }
            collection = digest(
                serde_json::json!({"kind":if runtime {"research.explanation-qualification.collection.v2"} else {"research.explanation-qualification.collection.v1"},"data":c}),
            )?;
            assessment = digest(
                serde_json::json!({"kind":if runtime {"research.explanation-qualification.assessment.v2"} else {"research.explanation-qualification.assessment.v1"},"evidence":evidence,"collection":collection,"preferences":report.preferences,"safety":report.safety.iter().map(|s|serde_json::json!({"id":s.id,"passed":s.passed})).collect::<Vec<_>>()}),
            )?;
        }
        Ok(Self {
            command,
            bytes: bytes.into(),
            collection,
            assessment,
        })
    }
    /// Exact command containing the explicit owner attestation reference.
    pub const fn command(&self) -> &Command {
        &self.command
    }
    /// Canonical retained bytes; rejected model responses are never retained.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Derived collection digest; empty for revocation.
    pub fn collection(&self) -> &str {
        &self.collection
    }
    /// Derived assessment digest; empty for revocation.
    pub fn assessment(&self) -> &str {
        &self.assessment
    }
}

/// Production-parity protocol pins for versioned runtime approval and owned execution.
/// Historical v1 approval retains its separate protocol and identities.
/// # Errors
/// Refuses frozen or preparation drift.
pub fn runtime_protocol() -> Result<(String, String, String), Error> {
    let suite = Suite::runtime().map_err(|_| Error)?;
    let cases=suite.cases().iter().map(|c| {
        let input=c.input().ok_or(Error)?;
        Ok(serde_json::json!({"id":c.id(),"case":input.identity(),"input":Hash::hash(input.prepared_input().ok_or(Error)?).as_str()}))
    }).collect::<Result<Vec<_>,Error>>()?;
    let probes = Suite::runtime_probe_inputs()
        .map_err(|_| Error)?
        .into_iter()
        .map(|(id, input)| serde_json::json!({"id":id,"input":Hash::hash(&input).as_str()}))
        .collect::<Vec<_>>();
    Ok((
        digest(
            serde_json::json!({"kind":"research.explanation-qualification.preparation.v2","suite":suite.identity(),"cases":cases,"contexts":probes}),
        )?,
        Hash::hash(crate::explanation_runtime::VALIDATOR_REVISION.as_bytes())
            .as_str()
            .into(),
        Hash::hash(crate::explanation_runtime::RENDERER_REVISION.as_bytes())
            .as_str()
            .into(),
    ))
}
