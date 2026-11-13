//! Shared production/qualification preparation over authenticated, reconciled owner inputs.
use crate::{
    explanation::{Allowlist, Error, ValidatedExplanation},
    explanation_context::{Permissions, Request},
    explanation_model::MAX_INPUT_BYTES,
    explanation_policy::PolicyPreview,
    explanation_preparation::CanonicalPreparation,
    explanation_reviewed_context::ReviewedContext,
    worker::ExecutionEvidence,
};
use edgeagent_contracts::{
    canonical_json_bytes, evidence::DailyEvidenceArtifact as Hash, explanation::Binding,
    explanation_profile::Profile, research::ResearchRecord, research_decision::DecisionProvenance,
};
/// Additive input contract; no fixture-only assertions or invented source template.
pub const INPUT_REVISION: &str = "research.explanation-input.v5";
/// Shared deterministic presentation revision, independent of historical evaluation renderers.
pub const RENDERER_REVISION: &str = "research-explanation-runtime.v1";
/// Existing strict typed proposal validator revision.
pub const VALIDATOR_REVISION: &str = "research-explanation-allowlist-v1";
const RANKING: &str =
    "ranking=raw_usd_ppm_descending_instrument_ascending; ties=instrument_ascending";
/// Current model-disclosure observations for an optional independently owned excerpt.
pub struct ModelContext<'a> {
    /// Canonical bounded document bytes, never instructions or a fact authority.
    pub bytes: &'a [u8],
    /// Exact anchor, audience, purpose and document identity.
    pub operation: &'a Request,
    /// Independent owner and Research artifact observations.
    pub artifact: &'a Permissions,
    /// Independent owner and Research document observations.
    pub document: &'a Permissions,
    /// Trusted current time for these observations.
    pub now_ms: i64,
}
/// Fresh serving observations; model-disclosure permission does not substitute.
pub struct ServingContext<'a> {
    /// Exact serving operation for the same excerpt and anchor.
    pub operation: &'a Request,
    /// Current independent artifact observations.
    pub artifact: &'a Permissions,
    /// Current independent document observations.
    pub document: &'a Permissions,
    /// Trusted current serving time.
    pub now_ms: i64,
}
/// Pure sealed snapshot. Caller authentication and pre-dispatch current rights remain mandatory.
pub struct RuntimePreparation<'a> {
    allowlist: Allowlist<'a>,
    policy: PolicyPreview<'a>,
    context: Option<ReviewedContext<'a>>,
    input: Vec<u8>,
}
/// Independently validated choices and deterministic shared rendering.
pub struct Presentation {
    validated: ValidatedExplanation,
    text: String,
}
impl Presentation {
    /// Existing sealed typed choices; no model text can populate facts.
    pub const fn validated(&self) -> &ValidatedExplanation {
        &self.validated
    }
    /// Exact rendering used by production preparation and the new frozen suite.
    pub fn rendered(&self) -> &str {
        &self.text
    }
}
impl<'a> RuntimePreparation<'a> {
    /// Authenticate retained state and owner evidence before calling. Reconcile all
    /// policy measurements; optional excerpts also require current purpose-bound grants.
    /// # Errors
    /// Refuses mismatched/incomplete inputs, unauthorized context and oversized data.
    pub fn from_verified(
        record: &'a ResearchRecord,
        decision: &'a DecisionProvenance,
        configuration: &[u8],
        evidence: &ExecutionEvidence,
        context: Option<ModelContext<'_>>,
    ) -> Result<Self, Error> {
        let allowlist = Allowlist::from_verified(record, decision)?;
        let prepared = CanonicalPreparation::from_verified(record, decision)?;
        let policy = PolicyPreview::from_verified(record, decision, configuration, evidence)?;
        let context = context
            .map(|c| {
                ReviewedContext::from_verified(
                    record,
                    decision,
                    c.bytes,
                    c.operation,
                    c.artifact,
                    c.document,
                    c.now_ms,
                )
            })
            .transpose()?;
        let reviewed = context
            .as_ref()
            .map(|c| serde_json::from_slice::<serde_json::Value>(c.input()).map_err(|_| Error))
            .transpose()?;
        let value = serde_json::json!({"kind":INPUT_REVISION,"policy_baseline":format!("{RENDERER_REVISION}\n{RANKING}\n{}",policy.standard()?),"prepared":serde_json::from_slice::<serde_json::Value>(prepared.input()).map_err(|_|Error)?,"reviewed_context":reviewed});
        let mut input = canonical_json_bytes(&value).map_err(|_| Error)?;
        input.push(b'\n');
        if input.len() > MAX_INPUT_BYTES {
            return Err(Error);
        }
        Ok(Self {
            allowlist,
            policy,
            context,
            input,
        })
    }
    /// Exact canonical model input; this snapshot is not a disclosure permit.
    pub fn input(&self) -> &[u8] {
        &self.input
    }
    /// SHA-256 of the actual prepared bytes, including LF.
    pub fn input_digest(&self) -> String {
        Hash::hash(&self.input).as_str().into()
    }
    /// Exact future worker binding for this shared protocol and immutable profile.
    /// # Errors
    /// Refuses malformed profiles or historical/different renderer and validator pins.
    pub fn binding(&self, profile: &Profile) -> Result<Binding, Error> {
        profile.canonical_bytes().map_err(|_| Error)?;
        if profile.renderer != Hash::hash(RENDERER_REVISION.as_bytes()).as_str()
            || profile.validator != Hash::hash(VALIDATOR_REVISION.as_bytes()).as_str()
        {
            return Err(Error);
        }
        Ok(Binding {
            anchor: self.allowlist.standard()?.anchor().clone(),
            profile: profile.id().map_err(|_| Error)?,
            evidence: self.input_digest(),
        })
    }
    /// Render the deterministic baseline with fresh serving observations for context.
    /// # Errors
    /// Refuses missing, stale, wrong-purpose or foreign context observations.
    pub fn standard(&self, serving: Option<ServingContext<'_>>) -> Result<Presentation, Error> {
        self.present(self.allowlist.standard()?, serving)
    }
    /// Validate only typed choices and reproduce the exact deterministic presentation.
    /// # Errors
    /// Refuses altered claims/citations or unavailable current context permissions.
    pub fn validate(
        &self,
        proposal: &[u8],
        serving: Option<ServingContext<'_>>,
    ) -> Result<Presentation, Error> {
        self.present(self.allowlist.validate(proposal)?, serving)
    }
    fn present(
        &self,
        validated: ValidatedExplanation,
        serving: Option<ServingContext<'_>>,
    ) -> Result<Presentation, Error> {
        let mut text = format!(
            "{RENDERER_REVISION}\n{RANKING}\n{}",
            self.policy
                .validate(&validated.proposal().canonical_bytes().map_err(|_| Error)?)?
        );
        match (&self.context, serving) {
            (Some(context), Some(s)) => {
                let document =
                    context.serving_document(s.operation, s.artifact, s.document, s.now_ms)?;
                let digest = s.operation.document.as_deref().ok_or(Error)?;
                text.push_str(&format!("context_digest={digest} input={}\nReviewed document context only; not evidence for ranking or policy; no tool or action authority.\ncontext_data={}",self.input_digest(),std::str::from_utf8(document).map_err(|_|Error)?));
            }
            (None, None) => {}
            _ => return Err(Error),
        }
        Ok(Presentation { validated, text })
    }
}
