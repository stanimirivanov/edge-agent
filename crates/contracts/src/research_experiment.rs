//! Versioned local experiment ledger inputs. Shape validation conveys no authority.
use crate::research::{
    EvidenceRef, MAX_CANDIDATES, MAX_RESEARCH_BYTES, ResearchRecord, ResearchRequest, digest, token,
};
use serde::{Deserialize, Serialize};

pub const MAX_TRIALS: usize = 64;
pub const MAX_COMMAND_BYTES: usize = 262_144;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidExperiment;
impl std::fmt::Display for InvalidExperiment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid research experiment ledger input")
    }
}
impl std::error::Error for InvalidExperiment {}

#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
enum ResultKind {
    #[serde(rename = "research.experiment.v1")]
    V1,
}

/// Exact experimental output, including its mandatory non-publication marker.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentResult {
    kind: ResultKind,
    publication_eligible: bool,
    result: Box<ResearchRecord>,
}
impl ExperimentResult {
    pub fn decode(bytes: &[u8]) -> Result<Self, InvalidExperiment> {
        decode(bytes, MAX_RESEARCH_BYTES + 128, Self::bytes)
    }
    pub fn record(&self) -> &ResearchRecord {
        &self.result
    }
    fn validate(&self) -> Result<(), InvalidExperiment> {
        if self.publication_eligible
            || !matches!(
                self.record(),
                ResearchRecord::Artifact(_) | ResearchRecord::Abstention(_)
            )
        {
            return Err(InvalidExperiment);
        }
        self.result
            .canonical_bytes()
            .map_err(|_| InvalidExperiment)?;
        Ok(())
    }
    fn bytes(&self) -> Result<Vec<u8>, InvalidExperiment> {
        self.validate()?;
        encode(self, MAX_RESEARCH_BYTES + 128)
    }
}

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    pub request: ResearchRequest,
    /// Exact ordered evidence census expected in a successful artifact.
    pub evidence: Vec<EvidenceRef>,
}
#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    InvalidInput,
    Unavailable,
    Cancelled,
}
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Terminal {
    Result { result: ExperimentResult },
    Failed { reason: Failure },
}
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Register {
        protocol: String,
        trials: Vec<Trial>,
    },
    Finish {
        trial: String,
        outcome: Terminal,
    },
    Select {
        trial: String,
    },
    Read,
}
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
pub enum Command {
    #[serde(rename = "research.experiment-ledger.v1")]
    V1 {
        scope: String,
        family: String,
        action: Action,
    },
}
impl Command {
    pub fn bytes(&self) -> Result<Vec<u8>, InvalidExperiment> {
        self.validate()?;
        encode(self, MAX_COMMAND_BYTES)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, InvalidExperiment> {
        decode(bytes, MAX_COMMAND_BYTES, Self::bytes)
    }
    fn validate(&self) -> Result<(), InvalidExperiment> {
        let Self::V1 {
            scope,
            family,
            action,
        } = self;
        if !token(scope) || !token(family) {
            return Err(InvalidExperiment);
        }
        match action {
            Action::Register { protocol, trials } => {
                if !digest(protocol)
                    || protocol.bytes().all(|b| b == b'0')
                    || !(1..=MAX_TRIALS).contains(&trials.len())
                    || !trials
                        .windows(2)
                        .all(|w| w[0].request.request_id < w[1].request.request_id)
                {
                    return Err(InvalidExperiment);
                }
                for trial in trials {
                    ResearchRecord::Request(trial.request.clone())
                        .canonical_bytes()
                        .map_err(|_| InvalidExperiment)?;
                    if trial.request.scope != *scope
                        || !(1..=MAX_CANDIDATES).contains(&trial.evidence.len())
                        || !trial.evidence.windows(2).all(|w| {
                            (&w[0].instrument, &w[0].digest) < (&w[1].instrument, &w[1].digest)
                        })
                        || trial.evidence.iter().any(|e| {
                            !trial.request.instruments.contains(&e.instrument)
                                || !digest(&e.digest)
                                || !(0..=trial.request.decision_at_ms).contains(&e.available_at_ms)
                        })
                    {
                        return Err(InvalidExperiment);
                    }
                }
            }
            Action::Finish { trial, outcome } => {
                if !token(trial) {
                    return Err(InvalidExperiment);
                }
                if let Terminal::Result { result } = outcome {
                    result.validate()?;
                }
            }
            Action::Select { trial } if !token(trial) => return Err(InvalidExperiment),
            Action::Select { .. } | Action::Read => {}
        }
        Ok(())
    }
}

fn encode(value: &impl Serialize, max: usize) -> Result<Vec<u8>, InvalidExperiment> {
    let mut bytes =
        crate::messaging::canonical_json::to_vec(value).map_err(|_| InvalidExperiment)?;
    bytes.push(b'\n');
    if bytes.len() > max {
        return Err(InvalidExperiment);
    }
    Ok(bytes)
}
fn decode<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    max: usize,
    canonical: impl FnOnce(&T) -> Result<Vec<u8>, InvalidExperiment>,
) -> Result<T, InvalidExperiment> {
    if bytes.len() > max {
        return Err(InvalidExperiment);
    }
    let value = serde_json::from_slice(bytes).map_err(|_| InvalidExperiment)?;
    if canonical(&value)? != bytes {
        return Err(InvalidExperiment);
    }
    Ok(value)
}
