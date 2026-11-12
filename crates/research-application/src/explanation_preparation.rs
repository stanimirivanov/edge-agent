//! Versioned shared preparation for future runtime and qualification consumers.
use crate::{
    explanation::{Allowlist, Error},
    explanation_model::MAX_INPUT_BYTES,
};
use edgeagent_contracts::{
    evidence::DailyEvidenceArtifact, explanation::Anchor, explanation_facts::Facts,
    research::ResearchRecord, research_decision::DecisionProvenance,
};

/// New input identity; never silently substituted for legacy worker/replay bytes.
pub const REVISION: &str = "research.explanation-input.v2";

/// Sealed canonical input. Only deterministic preparation can populate its facts.
/// No model dispatch, profile approval, owner authentication or disclosure permission.
pub struct CanonicalPreparation<'a> {
    facts: Facts<'a>,
    input: Vec<u8>,
}
impl<'a> CanonicalPreparation<'a> {
    /// Prepare the same exact facts/proposal bytes for runtime and qualification.
    /// The caller first authenticates retained state and verifies current owner evidence.
    /// # Errors
    /// Refuses inconsistent or unsupported inputs and oversized model input.
    pub fn from_verified(
        record: &'a ResearchRecord,
        decision: &'a DecisionProvenance,
    ) -> Result<Self, Error> {
        let facts = Facts::from_bound(record, decision).map_err(|_| Error)?;
        let standard = Allowlist::from_verified(record, decision)?.standard()?;
        let facts_bytes = facts.canonical_bytes().map_err(|_| Error)?;
        let proposal_bytes = standard.proposal().canonical_bytes().map_err(|_| Error)?;
        // Every nested value is already canonical. Fixed envelope keys are sorted too.
        let input = format!(
            "{{\"facts\":{},\"kind\":\"{REVISION}\",\"standard_proposal\":{},\"untrusted_context\":null}}\n",
            std::str::from_utf8(&facts_bytes).map_err(|_| Error)?.trim_end_matches('\n'),
            std::str::from_utf8(&proposal_bytes).map_err(|_| Error)?.trim_end_matches('\n'),
        ).into_bytes();
        if input.len() > MAX_INPUT_BYTES {
            return Err(Error);
        }
        Ok(Self { facts, input })
    }
    /// Read-only structured facts, not a source of owner rights.
    pub const fn facts(&self) -> &Facts<'a> {
        &self.facts
    }
    /// Exact original artifact, decision and scope.
    pub const fn anchor(&self) -> &Anchor {
        self.facts.anchor()
    }
    /// Exact versioned model input, including final LF.
    pub fn input(&self) -> &[u8] {
        &self.input
    }
    /// Evidence identity is the digest of the actual prepared bytes, not a suite ID.
    pub fn input_digest(&self) -> String {
        DailyEvidenceArtifact::hash(&self.input).as_str().into()
    }
}
