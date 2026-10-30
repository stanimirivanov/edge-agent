//! Pure staged eligibility composition. This result does not certify all M05 publication controls.
use crate::{EvaluationError, RenderedOutcome};
use edgeagent_contracts::research::{AbstentionReason, ResearchRecord, ResearchRequest};
use edgeagent_research_domain::{
    EvidencePolicy, FrozenSeries,
    policy::{Decision, Intent, InvalidInput, Policy, Status},
};
/// Governed evaluation failure; accepted-request denials instead return an abstention.
#[derive(Debug)]
pub enum Error {
    /// Existing evaluation failed.
    Evaluation(EvaluationError),
    /// Malformed policy intent.
    Policy(InvalidInput),
    /// Independent policy targets a different configuration.
    Binding,
    /// Exact liquidity calculation could not be represented.
    Liquidity(edgeagent_research_domain::liquidity::Error),
    /// Market constraint arithmetic cannot be represented.
    Market(edgeagent_research_domain::market_constraints::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("governed research evaluation refused")
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evaluation(e) => Some(e),
            Self::Policy(e) => Some(e),
            Self::Binding => None,
            Self::Liquidity(e) => Some(e),
            Self::Market(e) => Some(e),
        }
    }
}
/// Result of evidence evaluation plus these independent eligibility controls only.
pub struct GovernedOutcome {
    outcome: RenderedOutcome,
    decision: Option<Decision>,
}
impl GovernedOutcome {
    /// Immutable normal artifact or abstention with existing wire compatibility.
    #[must_use]
    pub const fn outcome(&self) -> &RenderedOutcome {
        &self.outcome
    }
    /// Eligibility decision; absent when required evidence already abstained.
    #[must_use]
    pub const fn decision(&self) -> Option<&Decision> {
        self.decision.as_ref()
    }
}
/// Explicit owner-bound evidence for independently selected market constraints.
#[derive(Clone, Copy)]
pub struct ConstraintInputs<'a> {
    /// Complete volume assertions when liquidity is selected.
    pub volumes: &'a [edgeagent_research_domain::liquidity::Input],
    /// Complete raw opening/action assertions when volatility/gaps are selected.
    pub opens: &'a [edgeagent_research_domain::market_constraints::Input],
}
/// Explicit context and prior market inputs; no new fields are added to older input contracts.
pub struct ContextInputs<'a> {
    /// Existing independent volume/open evidence.
    pub constraints: ConstraintInputs<'a>,
    /// Owner-bound event context for every required instrument.
    pub context: &'a [edgeagent_research_domain::context_policy::Input],
}
/// Governed evaluation with critical event/regime/quality context.
/// # Errors
/// Preserves prior errors and selected constraints; missing context abstains.
pub fn evaluate_with_context_inputs(
    request: ResearchRequest,
    evidence: &EvidencePolicy,
    inputs: &[FrozenSeries],
    expires_at_ms: i64,
    policy: &Policy,
    intent: Intent,
    context: ContextInputs<'_>,
) -> Result<GovernedOutcome, Error> {
    evaluate_inner(
        request,
        evidence,
        inputs,
        expires_at_ms,
        policy,
        intent,
        context,
    )
}
/// Governed evaluation with explicit evidence for every selected constraint.
/// # Errors
/// Preserves calculation/binding errors; selected missing evidence cannot bypass a gate.
pub fn evaluate_with_market_inputs(
    request: ResearchRequest,
    evidence: &EvidencePolicy,
    inputs: &[FrozenSeries],
    expires_at_ms: i64,
    policy: &Policy,
    intent: Intent,
    constraints: ConstraintInputs<'_>,
) -> Result<GovernedOutcome, Error> {
    evaluate_inner(
        request,
        evidence,
        inputs,
        expires_at_ms,
        policy,
        intent,
        ContextInputs {
            constraints,
            context: &[],
        },
    )
}
/// Validate the full required evidence before narrowing; preserve scores and complete lineage.
/// Direction/horizon are explicit trusted intent inputs, never inferred from score sign or TTL.
/// # Errors
/// Rejects malformed intent, configuration mismatch and existing evaluation failures.
pub fn evaluate(
    request: ResearchRequest,
    evidence: &EvidencePolicy,
    inputs: &[FrozenSeries],
    expires_at_ms: i64,
    policy: &Policy,
    intent: Intent,
) -> Result<GovernedOutcome, Error> {
    evaluate_inner(
        request,
        evidence,
        inputs,
        expires_at_ms,
        policy,
        intent,
        ContextInputs {
            constraints: ConstraintInputs {
                volumes: &[],
                opens: &[],
            },
            context: &[],
        },
    )
}
/// Governed evaluation with owner-bound volume rows; selected liquidity is never optional.
/// # Errors
/// Preserves ordinary errors and rejects unrepresentable liquidity without partial results.
pub fn evaluate_with_liquidity(
    request: ResearchRequest,
    evidence: &EvidencePolicy,
    inputs: &[FrozenSeries],
    expires_at_ms: i64,
    policy: &Policy,
    intent: Intent,
    volumes: &[edgeagent_research_domain::liquidity::Input],
) -> Result<GovernedOutcome, Error> {
    evaluate_inner(
        request,
        evidence,
        inputs,
        expires_at_ms,
        policy,
        intent,
        ContextInputs {
            constraints: ConstraintInputs {
                volumes,
                opens: &[],
            },
            context: &[],
        },
    )
}
fn evaluate_inner(
    request: ResearchRequest,
    evidence: &EvidencePolicy,
    inputs: &[FrozenSeries],
    expires_at_ms: i64,
    policy: &Policy,
    intent: Intent,
    context: ContextInputs<'_>,
) -> Result<GovernedOutcome, Error> {
    let constraints = context.constraints;
    let contexts = context.context;
    let volumes = constraints.volumes;
    let expected: String = policy
        .configuration()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if request.configuration != expected {
        return Err(Error::Binding);
    }
    let candidates = evidence
        .universe()
        .iter()
        .map(|s| s.instrument)
        .collect::<Vec<_>>();
    // Validate intent even if evidence will abstain; retain no apparent decision in that case.
    let mut decision = policy.assess(&candidates, intent).map_err(Error::Policy)?;
    let base = crate::evaluate(request.clone(), evidence, inputs, expires_at_ms)
        .map_err(Error::Evaluation)?;
    let ResearchRecord::Artifact(artifact) = base.record() else {
        return Ok(GovernedOutcome {
            outcome: base,
            decision: None,
        });
    };
    if policy.requires_liquidity() {
        use edgeagent_research_domain::liquidity::{self, Error as L};
        let assessed = (|| {
            if volumes.len() > 64
                || volumes.iter().enumerate().any(|(i, v)| {
                    volumes[..i]
                        .iter()
                        .any(|old| old.instrument == v.instrument)
                })
                || volumes.iter().any(|v| !candidates.contains(&v.instrument))
            {
                return Err(L::InvalidEvidence);
            }
            let eligible = evidence.assess(inputs).map_err(|_| L::InvalidEvidence)?;
            eligible
                .series()
                .iter()
                .map(|s| {
                    let input = volumes
                        .iter()
                        .find(|v| v.instrument == s.frozen().selected.instrument)
                        .ok_or(L::InsufficientHistory)?;
                    liquidity::calculate(s, input)
                })
                .collect::<Result<Vec<_>, L>>()
        })();
        match assessed {
            Ok(values) => {
                decision = policy
                    .apply_liquidity(decision, &values)
                    .map_err(Error::Policy)?
            }
            Err(L::OutOfRange) => return Err(Error::Liquidity(L::OutOfRange)),
            Err(error) => {
                return Ok(GovernedOutcome {
                    outcome: crate::abstain(
                        request,
                        if error == L::InsufficientHistory {
                            AbstentionReason::InsufficientHistory
                        } else {
                            AbstentionReason::InvalidEvidence
                        },
                    )
                    .map_err(Error::Evaluation)?,
                    decision: None,
                });
            }
        }
    }
    if policy.requires_market_constraints() {
        use edgeagent_research_domain::market_constraints::{self, Error as M};
        let assessed = (|| {
            let opens = constraints.opens;
            if opens.len() > 64
                || opens
                    .iter()
                    .enumerate()
                    .any(|(i, v)| opens[..i].iter().any(|old| old.instrument == v.instrument))
                || opens.iter().any(|v| !candidates.contains(&v.instrument))
            {
                return Err(M::InvalidEvidence);
            }
            let eligible = evidence.assess(inputs).map_err(|_| M::InvalidEvidence)?;
            eligible
                .series()
                .iter()
                .map(|s| {
                    let input = opens
                        .iter()
                        .find(|v| v.instrument == s.frozen().selected.instrument)
                        .ok_or(M::InsufficientHistory)?;
                    market_constraints::calculate(s, input)
                })
                .collect::<Result<Vec<_>, M>>()
        })();
        match assessed {
            Ok(values) => {
                decision = policy
                    .apply_market_constraints(decision, &values)
                    .map_err(Error::Policy)?
            }
            Err(error @ (M::OutOfRange | M::Return(_))) => return Err(Error::Market(error)),
            Err(error) => {
                return Ok(GovernedOutcome {
                    outcome: crate::abstain(
                        request,
                        if error == M::InsufficientHistory {
                            AbstentionReason::InsufficientHistory
                        } else {
                            AbstentionReason::InvalidEvidence
                        },
                    )
                    .map_err(Error::Evaluation)?,
                    decision: None,
                });
            }
        }
    }
    if let Some(rules) = policy.context() {
        use edgeagent_research_domain::context_policy::Error as C;
        let assessed = (|| {
            if contexts.len() > 64
                || contexts.iter().enumerate().any(|(i, v)| {
                    contexts[..i]
                        .iter()
                        .any(|old| old.instrument == v.instrument)
                })
                || contexts.iter().any(|v| !candidates.contains(&v.instrument))
            {
                return Err(C::InvalidEvidence);
            }
            let eligible = evidence.assess(inputs).map_err(|_| C::InvalidEvidence)?;
            let mut failure: Option<C> = None;
            let mut values = Vec::new();
            for s in eligible.series() {
                let result = contexts
                    .iter()
                    .find(|v| v.instrument == s.frozen().selected.instrument)
                    .ok_or(C::CoverageUnavailable)
                    .and_then(|v| rules.assess(s, v, request.decision_at_ms));
                match result {
                    Ok(value) => values.push(value),
                    Err(error) => failure = Some(failure.map_or(error, |old| old.min(error))),
                }
            }
            if let Some(error) = failure {
                Err(error)
            } else {
                Ok(values)
            }
        })();
        match assessed {
            Ok(values) => {
                decision = policy
                    .apply_context(decision, &values)
                    .map_err(Error::Policy)?
            }
            Err(error) => {
                return Ok(GovernedOutcome {
                    outcome: crate::abstain(
                        request,
                        match error {
                            C::MissingEntitlement => AbstentionReason::MissingEntitlement,
                            C::InvalidEvidence => AbstentionReason::InvalidEvidence,
                            C::FutureEvidence => AbstentionReason::FutureEvidence,
                            C::StaleEvidence => AbstentionReason::StaleEvidence,
                            C::CoverageUnavailable => AbstentionReason::InsufficientHistory,
                        },
                    )
                    .map_err(Error::Evaluation)?,
                    decision: None,
                });
            }
        }
    }
    let outcome = match decision.status() {
        Status::Approved => base,
        Status::Abstained => crate::abstain(request, AbstentionReason::NoEligibleCandidate)
            .map_err(Error::Evaluation)?,
        Status::Narrowed => {
            let mut narrowed = artifact.clone();
            narrowed.ranking.retain(|r| {
                decision
                    .eligible()
                    .iter()
                    .any(|id| r.instrument == format!("{:032x}", id.get()))
            });
            crate::finish(ResearchRecord::Artifact(narrowed)).map_err(Error::Evaluation)?
        }
    };
    Ok(GovernedOutcome {
        outcome,
        decision: Some(decision),
    })
}
