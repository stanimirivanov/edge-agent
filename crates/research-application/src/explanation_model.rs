//! Consumer-owned model port. Authority and proposal validation stay outside inference.
use edgeagent_contracts::{
    explanation::{Binding, MAX_BYTES},
    explanation_profile::{Deployment, Profile},
};
use std::fmt;

/// Maximum bytes in the prepared model input before transport encoding.
pub const MAX_INPUT_BYTES: usize = 65_536;

/// Payload-safe outcomes for inference orchestration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    /// Prepared input or its profile binding is invalid.
    InvalidRequest,
    /// The model endpoint cannot be reached or used.
    Unavailable,
    /// The request deadline elapsed.
    Timeout,
    /// The model refused to produce a proposal.
    Refused,
    /// No inference budget remains.
    BudgetExhausted,
    /// Observed deployment does not match the requested pins.
    DeploymentMismatch,
    /// Output framing or size is invalid.
    InvalidResponse,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRequest => "invalid model request",
            Self::Unavailable => "model unavailable",
            Self::Timeout => "model deadline elapsed",
            Self::Refused => "model refused",
            Self::BudgetExhausted => "model budget exhausted",
            Self::DeploymentMismatch => "model deployment mismatch",
            Self::InvalidResponse => "invalid model response",
        })
    }
}
impl std::error::Error for Failure {}

/// Bounded request bound to an exact explanation profile and evidence set.
pub struct Request<'a> {
    profile: &'a Profile,
    binding: &'a Binding,
    input: &'a [u8],
}

impl<'a> Request<'a> {
    /// Bind input prepared and authorized by the caller.
    ///
    /// # Errors
    /// Rejects empty or oversized input and invalid or mismatched identities.
    pub fn new(
        profile: &'a Profile,
        binding: &'a Binding,
        input: &'a [u8],
    ) -> Result<Self, Failure> {
        if input.is_empty()
            || input.len() > MAX_INPUT_BYTES
            || !binding.is_valid()
            || profile.id().map_err(|_| Failure::InvalidRequest)? != binding.profile
        {
            return Err(Failure::InvalidRequest);
        }
        Ok(Self {
            profile,
            binding,
            input,
        })
    }

    /// Exact requested profile.
    pub const fn profile(&self) -> &Profile {
        self.profile
    }

    /// Artifact, decision, evidence and profile identities for this request.
    pub const fn binding(&self) -> &Binding {
        self.binding
    }

    /// Prepared model data.
    pub const fn input(&self) -> &[u8] {
        self.input
    }

    /// Match independently observed deployment identity and bind bounded output.
    /// Adapters obtain the observation through a trusted deployment check and
    /// enforce bounds while reading. A response's self-reported ID is insufficient.
    ///
    /// # Errors
    /// Rejects mismatched deployment and empty or oversized proposal bytes.
    pub fn accept(&self, observed: Deployment, proposal: Vec<u8>) -> Result<Reply, Failure> {
        if observed != self.profile.deployment {
            return Err(Failure::DeploymentMismatch);
        }
        if proposal.is_empty() || proposal.len() > MAX_BYTES {
            return Err(Failure::InvalidResponse);
        }
        Ok(Reply {
            binding: self.binding.clone(),
            observed,
            proposal,
        })
    }
}

/// Bounded untrusted proposal with its request binding and deployment observation.
pub struct Reply {
    binding: Binding,
    observed: Deployment,
    proposal: Vec<u8>,
}

impl Reply {
    /// Original request binding.
    pub const fn binding(&self) -> &Binding {
        &self.binding
    }

    /// Independently observed identity supplied by the trusted adapter.
    pub const fn observed_deployment(&self) -> &Deployment {
        &self.observed
    }

    /// Untrusted bytes for the independent proposal validator.
    pub fn proposal(&self) -> &[u8] {
        &self.proposal
    }
}

/// Model-neutral inference capability owned by Research.
pub trait ExplanationModel: Sync {
    /// Obtain one bounded proposal under the requested profile.
    fn propose(&self, request: &Request<'_>)
    -> impl Future<Output = Result<Reply, Failure>> + Send;
}
#[cfg(test)]
mod tests {
    use super::*;
    use edgeagent_contracts::{
        explanation::Anchor,
        explanation_profile::{Settings, Tools},
    };

    #[test]
    fn request_and_reply_check_bounds_binding_and_deployment()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = Profile {
            deployment: Deployment {
                runtime: "a".repeat(64),
                weights: "b".repeat(64),
            },
            settings: Settings {
                seed: 1,
                temperature_milli: 0,
                max_output_tokens: 256,
                timeout_ms: 1000,
            },
            prompt: "c".repeat(64),
            validator: "d".repeat(64),
            renderer: "e".repeat(64),
            tools: Tools::None,
        };
        let binding = Binding {
            anchor: Anchor {
                scope: "operator".into(),
                artifact: "a".repeat(64),
                decision: "b".repeat(64),
            },
            profile: profile.id()?,
            evidence: "c".repeat(64),
        };
        let request = Request::new(&profile, &binding, b"approved input")?;
        assert_eq!(request.input(), b"approved input");
        assert_eq!(request.profile(), &profile);
        assert!(Request::new(&profile, &binding, &[]).is_err());
        assert!(Request::new(&profile, &binding, &vec![b'a'; MAX_INPUT_BYTES + 1]).is_err());
        let mismatched = Binding {
            profile: "f".repeat(64),
            ..binding.clone()
        };
        assert!(Request::new(&profile, &mismatched, b"input").is_err());
        let observed = Deployment {
            weights: "f".repeat(64),
            ..profile.deployment.clone()
        };
        assert!(matches!(
            request.accept(observed, vec![b'a']),
            Err(Failure::DeploymentMismatch)
        ));
        assert!(matches!(
            request.accept(profile.deployment.clone(), vec![0; MAX_BYTES + 1]),
            Err(Failure::InvalidResponse)
        ));
        let reply = request.accept(profile.deployment.clone(), b"untrusted proposal".to_vec())?;
        assert_eq!(reply.proposal(), b"untrusted proposal");
        assert_eq!(reply.binding().profile, binding.profile);
        assert_eq!(reply.observed_deployment(), &profile.deployment);
        Ok(())
    }
}
