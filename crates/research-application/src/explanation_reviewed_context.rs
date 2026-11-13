//! Purpose-bound reviewed context for future qualification; no retrieval or model dispatch.
use crate::{
    explanation::Error,
    explanation_context::{AuthorizedExcerpt, Permissions, Purpose, Request},
    explanation_intent::Preview,
    explanation_model::MAX_INPUT_BYTES,
    explanation_preparation::CanonicalPreparation,
};
use edgeagent_contracts::{
    evidence::DailyEvidenceArtifact, explanation::Anchor, research::ResearchRecord,
    research_decision::DecisionProvenance,
};
/// Additive input revision: the existing preparation is nested without changing its bytes.
pub const INPUT_REVISION: &str = "research.explanation-input.v3";
/// Fixed reviewed-context presentation, separate from all historical renderer pins.
pub const RENDERER_REVISION: &str = "research-explanation-reviewed-context.v1";
/// Integrity protocol for reader-task inputs and baseline; never a qualification result.
pub const CASE_REVISION: &str = "reviewed-context-reader-case.v1";

/// Checked immutable document snapshot. Current permission must be rechecked before
/// dispatch; presentation checks both artifact and document permissions on every call.
pub struct ReviewedContext<'a> {
    prepared: CanonicalPreparation<'a>,
    preview: Preview<'a>,
    excerpt: AuthorizedExcerpt,
    document: Vec<u8>,
    input: Vec<u8>,
}
fn allowed(
    operation: &Request,
    artifact: &Permissions,
    document: &Permissions,
    purpose: Purpose,
    anchor: &Anchor,
    digest: &str,
    now_ms: i64,
) -> bool {
    operation.purpose == purpose
        && &operation.anchor == anchor
        && operation.document.as_deref() == Some(digest)
        && artifact.allows(
            &Request {
                document: None,
                ..operation.clone()
            },
            now_ms,
        )
        && document.allows(operation, now_ms)
}
impl<'a> ReviewedContext<'a> {
    /// Caller authenticates retained state, owner evidence and current permission observations.
    /// # Errors
    /// Refuses foreign, unreviewed, future, malformed, oversized or unauthorized context.
    pub fn from_verified(
        record: &'a ResearchRecord,
        decision: &'a DecisionProvenance,
        bytes: &[u8],
        operation: &Request,
        artifact: &Permissions,
        document: &Permissions,
        now_ms: i64,
    ) -> Result<Self, Error> {
        let prepared = CanonicalPreparation::from_verified(record, decision)?;
        let excerpt = AuthorizedExcerpt::decode(
            bytes,
            operation,
            document,
            decision.request.decision_at_ms,
            now_ms,
        )
        .map_err(|_| Error)?;
        if !allowed(
            operation,
            artifact,
            document,
            Purpose::ModelDisclosure,
            prepared.anchor(),
            excerpt.digest(),
            now_ms,
        ) {
            return Err(Error);
        }
        // All nested values are canonical; audience is a checked ASCII token.
        let input=format!("{{\"kind\":\"{INPUT_REVISION}\",\"prepared\":{},\"untrusted_context\":{{\"audience\":\"{}\",\"digest\":\"{}\",\"document\":{},\"purpose\":\"model_disclosure\"}}}}\n",
            std::str::from_utf8(prepared.input()).map_err(|_|Error)?.trim_end_matches('\n'),operation.audience,excerpt.digest(),
            std::str::from_utf8(bytes).map_err(|_|Error)?.trim_end_matches('\n')).into_bytes();
        if input.len() > MAX_INPUT_BYTES {
            return Err(Error);
        }
        Ok(Self {
            prepared,
            preview: Preview::from_verified(record, decision)?,
            excerpt,
            document: bytes.to_vec(),
            input,
        })
    }
    /// Exact canonical data snapshot, not an authorization to disclose or dispatch.
    pub fn input(&self) -> &[u8] {
        &self.input
    }
    /// Identity of actual prepared bytes, including the exact document and model audience.
    pub fn input_digest(&self) -> String {
        DailyEvidenceArtifact::hash(&self.input).as_str().into()
    }
    /// Fixed baseline after a fresh artifact-and-document serving check.
    /// # Errors
    /// Refuses unavailable, revoked, expired, wrong-purpose or foreign permission observations.
    pub fn standard(
        &self,
        operation: &Request,
        artifact: &Permissions,
        document: &Permissions,
        now_ms: i64,
    ) -> Result<String, Error> {
        self.present(
            &self.preview.standard()?,
            operation,
            artifact,
            document,
            now_ms,
        )
    }
    /// Validate typed choices; documents can neither supply claims nor alter the fixed data.
    /// # Errors
    /// Refuses invalid proposals or missing current serving permission for either subject.
    pub fn validate(
        &self,
        proposal: &[u8],
        operation: &Request,
        artifact: &Permissions,
        document: &Permissions,
        now_ms: i64,
    ) -> Result<String, Error> {
        self.present(
            &self.preview.validate(proposal)?,
            operation,
            artifact,
            document,
            now_ms,
        )
    }
    fn present(
        &self,
        baseline: &str,
        operation: &Request,
        artifact: &Permissions,
        document: &Permissions,
        now_ms: i64,
    ) -> Result<String, Error> {
        let anchor = self.prepared.anchor();
        if now_ms < self.prepared.facts().request().decision_at_ms {
            return Err(Error);
        }
        if !allowed(
            operation,
            artifact,
            document,
            Purpose::ExplanationServing,
            anchor,
            self.excerpt.digest(),
            now_ms,
        ) {
            return Err(Error);
        }
        let (_, body) = baseline.split_once('\n').ok_or(Error)?;
        // Canonical JSON quotes newline/tab content, preventing forged presentation rows.
        Ok(format!(
            "{RENDERER_REVISION} artifact={} decision={} context_digest={} input={}\n{body}Reviewed document context only; not evidence for ranking or policy; no tool or action authority.\ncontext_data={}",
            anchor.artifact,
            anchor.decision,
            self.excerpt.digest(),
            self.input_digest(),
            std::str::from_utf8(&self.document).map_err(|_| Error)?
        ))
    }
    /// Pin the exact baseline, reader task, input and serving audience for a future suite.
    /// This is an integrity identity, not a passing score or generation approval.
    /// # Errors
    /// Refuses empty/oversized tasks or unavailable current serving permissions.
    pub fn case_identity(
        &self,
        task: &str,
        operation: &Request,
        artifact: &Permissions,
        document: &Permissions,
        now_ms: i64,
    ) -> Result<String, Error> {
        if task.is_empty() || task.len() > 4096 {
            return Err(Error);
        }
        let baseline = self.standard(operation, artifact, document, now_ms)?;
        let mut bytes = Vec::new();
        for part in [
            CASE_REVISION.as_bytes(),
            INPUT_REVISION.as_bytes(),
            RENDERER_REVISION.as_bytes(),
            self.input(),
            task.as_bytes(),
            operation.audience.as_bytes(),
            operation.purpose.code().as_bytes(),
            baseline.as_bytes(),
        ] {
            bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
            bytes.extend_from_slice(part);
        }
        Ok(DailyEvidenceArtifact::hash(&bytes).as_str().into())
    }
}
