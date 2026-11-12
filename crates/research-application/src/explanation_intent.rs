//! Opt-in deterministic v2 preview. No model dispatch, persistence or disclosure authority.
use crate::explanation::{Allowlist, Error, ValidatedExplanation};
use edgeagent_contracts::{
    explanation_facts::Facts,
    research::ResearchRecord,
    research_decision::{DecisionProvenance, Direction},
};
use std::fmt::Write as _;

/// Distinct from legacy rendering, preparation and frozen evaluation revisions.
pub const REVISION: &str = "research-explanation-trace.v2";
/// Verified retained facts plus the existing strict proposal boundary.
pub struct Preview<'a> {
    facts: Facts<'a>,
    allowlist: Allowlist<'a>,
}
impl<'a> Preview<'a> {
    /// Caller authenticates retained state and independently verifies owner evidence first.
    /// # Errors
    /// Refuses inconsistent decisions, abstentions, or missing supporting evidence.
    pub fn from_verified(
        record: &'a ResearchRecord,
        decision: &'a DecisionProvenance,
    ) -> Result<Self, Error> {
        Ok(Self {
            facts: Facts::from_bound(record, decision).map_err(|_| Error)?,
            allowlist: Allowlist::from_verified(record, decision)?,
        })
    }
    /// Render the complete deterministic baseline, including every required limitation.
    /// # Errors
    /// Refuses an unsupported or unrenderable fact set.
    pub fn standard(&self) -> Result<String, Error> {
        self.render(&self.allowlist.standard()?)
    }
    /// Validate typed presentation choices; facts and required disclosures stay fixed.
    /// Replaying accepted canonical proposal bytes yields identical text without inference.
    /// # Errors
    /// Refuses unsupported, altered, missing, duplicated or reordered claims/citations.
    pub fn validate(&self, proposal: &[u8]) -> Result<String, Error> {
        self.render(&self.allowlist.validate(proposal)?)
    }
    fn render(&self, baseline: &ValidatedExplanation) -> Result<String, Error> {
        let facts = &self.facts;
        let request = facts.request();
        let decision = facts.decision();
        let anchor = facts.anchor();
        let direction = match decision.direction {
            Direction::Long => "long",
            Direction::Short => "short",
        };
        let mut text = format!(
            "{REVISION} artifact={} decision={}\n",
            anchor.artifact, anchor.decision
        );
        writeln!(
            text,
            "scope={} universe={} implementation={} configuration={} artifact={}",
            request.scope,
            request.instruments.join(","),
            request.implementation,
            request.configuration,
            anchor.artifact
        )
        .map_err(|_| Error)?;
        writeln!(text, "direction={direction} holding_horizon_sessions={} measurement_lookback_sessions={} policy={} decision={}",
            decision.horizon_sessions, request.lookback_sessions, decision.policy, anchor.decision).map_err(|_| Error)?;
        writeln!(
            text,
            "decision_at_ms={} artifact_expires_at_ms={} artifact={}",
            request.decision_at_ms,
            facts.expires_at_ms(),
            anchor.artifact
        )
        .map_err(|_| Error)?;
        for source in facts.evidence() {
            let age = request
                .decision_at_ms
                .checked_sub(source.available_at_ms)
                .ok_or(Error)?;
            writeln!(text, "instrument={} evidence={} available_at_ms={} age_at_decision_ms={age} event_at_ms=unavailable observed_at_ms=unavailable",
                source.instrument, source.digest, source.available_at_ms).map_err(|_| Error)?;
        }
        // Reuse the immutable v1 rank/reason templates and validated research-only
        // disclosure; replace only their v1 header with this renderer's own header.
        let (_, body) = baseline.rendered().split_once('\n').ok_or(Error)?;
        text.push_str(body);
        text.push_str("Historical evidence at the decision cutoff; not a current-market quote.\nScenarios and risk illustrations: not qualified.\n");
        Ok(text)
    }
}
