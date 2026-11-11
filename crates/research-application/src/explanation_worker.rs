//! Prepared explanation inputs and sealed model/fallback results. I/O authority stays with the caller.
use crate::{
    explanation::{Allowlist, Error, ValidatedExplanation},
    explanation_model::{ExplanationModel, Failure, Request},
};
use edgeagent_contracts::{
    evidence::DailyEvidenceArtifact,
    explanation::{Binding, Fallback},
    explanation_profile::Profile,
    research::ResearchRecord,
    research_decision::DecisionProvenance,
};

/// One prepared input, bound to the independently verified retained decision.
pub struct Prepared<'a> {
    allowlist: Allowlist<'a>,
    binding: Binding,
    input: Vec<u8>,
}
/// Only independently validated choices can reach terminal persistence.
pub struct Resolved {
    /// Fixed rendering from the verified allowlist.
    pub explanation: ValidatedExplanation,
    /// None denotes a validated model proposal.
    pub reason: Option<Fallback>,
}
impl<'a> Prepared<'a> {
    /// Build exact input after authenticating retained state and current owner evidence.
    /// # Errors
    /// Refuses inconsistent records, invalid profiles, or unrenderable facts.
    pub fn new(
        record: &'a ResearchRecord,
        decision: &'a DecisionProvenance,
        profile: &Profile,
    ) -> Result<Self, Error> {
        if profile.validator
            != DailyEvidenceArtifact::hash(b"research-explanation-allowlist-v1").as_str()
            || profile.renderer
                != DailyEvidenceArtifact::hash(b"research-explanation-trace.v1").as_str()
        {
            return Err(Error);
        }
        let allowlist = Allowlist::from_verified(record, decision)?;
        let standard = allowlist.standard()?;
        let input = format!(
            "{{\"artifact\":{},\"decision\":{},\"standard_proposal\":{},\"untrusted_context\":null}}",
            std::str::from_utf8(&record.canonical_bytes().map_err(|_| Error)?)
                .map_err(|_| Error)?.trim_end_matches('\n'),
            std::str::from_utf8(&decision.canonical_bytes().map_err(|_| Error)?)
                .map_err(|_| Error)?.trim_end_matches('\n'),
            std::str::from_utf8(&standard.proposal().canonical_bytes().map_err(|_| Error)?)
                .map_err(|_| Error)?.trim_end_matches('\n'),
        )
        .into_bytes();
        let binding = Binding {
            anchor: standard.anchor().clone(),
            profile: profile.id().map_err(|_| Error)?,
            evidence: DailyEvidenceArtifact::hash(&input).as_str().into(),
        };
        Request::new(profile, &binding, &input).map_err(|_| Error)?;
        Ok(Self {
            allowlist,
            binding,
            input,
        })
    }
    /// Exact profile, artifact, decision and prepared evidence identity.
    pub const fn binding(&self) -> &Binding {
        &self.binding
    }
    /// Exact synthetic qualification-compatible model input, without disclosure authority.
    pub fn input(&self) -> &[u8] {
        &self.input
    }
    /// Exact model input digest for the durable reservation.
    pub fn input_digest(&self) -> String {
        DailyEvidenceArtifact::hash(&self.input).as_str().into()
    }
    /// Safe deterministic path while qualification or model disclosure is unavailable.
    /// # Errors
    /// Refuses an unrenderable allowlist.
    pub fn fallback(&self, reason: Fallback) -> Result<Resolved, Error> {
        Ok(Resolved {
            explanation: self.allowlist.standard()?,
            reason: Some(reason),
        })
    }
    /// Generation remains disabled until an independently reviewed qualification passes.
    /// # Errors
    /// Refuses an unrenderable allowlist.
    pub fn disabled(&self) -> Result<Resolved, Error> {
        self.fallback(Fallback::Disabled)
    }
    /// Invoke once after durable reservation and current disclosure checks.
    /// # Errors
    /// Refuses a profile differing from the prepared binding or an unrenderable fallback.
    pub async fn infer(
        &self,
        profile: &Profile,
        model: &impl ExplanationModel,
    ) -> Result<Resolved, Error> {
        let request = Request::new(profile, &self.binding, &self.input).map_err(|_| Error)?;
        let reason = match model.propose(&request).await {
            Ok(reply)
                if reply.binding().anchor == self.binding.anchor
                    && reply.binding().profile == self.binding.profile
                    && reply.binding().evidence == self.binding.evidence
                    && reply.observed_deployment() == &profile.deployment =>
            {
                if let Ok(explanation) = self.allowlist.validate(reply.proposal()) {
                    return Ok(Resolved {
                        explanation,
                        reason: None,
                    });
                }
                Fallback::InvalidProposal
            }
            Ok(_) => Fallback::InvalidProposal,
            Err(Failure::Timeout) => Fallback::Timeout,
            Err(Failure::BudgetExhausted) => Fallback::BudgetExhausted,
            Err(Failure::InvalidResponse) => Fallback::InvalidProposal,
            Err(_) => Fallback::Unavailable,
        };
        self.fallback(reason)
    }
}
