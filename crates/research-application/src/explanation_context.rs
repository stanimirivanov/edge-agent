//! Purpose-bound disclosure checks and inert document context.
use edgeagent_contracts::explanation::{Anchor, DocumentExcerpt};

/// Independent uses of the same evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Purpose {
    /// Use evidence in Research.
    ResearchUse,
    /// Send evidence to the selected model audience.
    ModelDisclosure,
    /// Serve an explanation to the selected reader audience.
    ExplanationServing,
}

impl Purpose {
    /// Stable code for owner grant storage and commands.
    pub const fn code(self) -> &'static str {
        match self {
            Self::ResearchUse => "research_use",
            Self::ModelDisclosure => "model_disclosure",
            Self::ExplanationServing => "explanation_serving",
        }
    }
}

/// Exact authorization subject. All fields come from the authenticated
/// operation and its reviewed inputs.
#[derive(Clone, Eq, PartialEq)]
pub struct Request {
    /// Artifact, decision and caller scope.
    pub anchor: Anchor,
    /// Operator-defined audience label, never an endpoint or executable.
    pub audience: String,
    /// One operation purpose.
    pub purpose: Purpose,
    /// Reviewed excerpt digest when access is for an individual document.
    pub document: Option<String>,
}

impl Request {
    /// Check the operation identity without consulting any authority.
    pub fn is_valid(&self) -> bool {
        self.anchor.is_valid()
            && (1..=64).contains(&self.audience.len())
            && self
                .audience
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            && self.document.as_ref().is_none_or(|id| {
                id.len() == 64
                    && id.bytes().any(|b| b != b'0')
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
    }
}

/// Current grant observation from an authenticated authority.
#[derive(Clone)]
pub struct Grant {
    /// Exact operation this grant authorizes.
    pub request: Request,
    /// Inclusive first millisecond.
    pub valid_from_ms: i64,
    /// Exclusive expiry.
    pub expires_at_ms: i64,
    /// Revocation takes precedence over the validity interval.
    pub revoked: bool,
}

impl Grant {
    fn allows(&self, request: &Request, now_ms: i64) -> bool {
        !self.revoked
            && &self.request == request
            && self.valid_from_ms >= 0
            && self.valid_from_ms <= now_ms
            && now_ms < self.expires_at_ms
    }
}

/// Current observations from the evidence owner and Research.
/// Missing or unavailable observations give no permission.
#[derive(Default)]
pub struct Permissions {
    /// Evidence-owner rights for this operation.
    pub owner: Option<Grant>,
    /// Research grant, which can only narrow evidence-owner rights.
    pub research: Option<Grant>,
}

impl Permissions {
    /// Require both authorities to allow this exact operation at the check time.
    pub fn allows(&self, request: &Request, now_ms: i64) -> bool {
        request.is_valid()
            && [&self.owner, &self.research].into_iter().all(|grant| {
                grant
                    .as_ref()
                    .is_some_and(|grant| grant.allows(request, now_ms))
            })
    }
}

/// Consumer-owned authority port. Implementations authenticate their sources
/// and resolve the current observations for each check, including revocations.
pub trait DisclosureAuthority: Sync {
    /// Read current permissions. Unavailable sources return absent grants.
    fn current(&self, request: &Request) -> impl Future<Output = Permissions> + Send;
}

/// Permitted generation path at this check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Model disclosure and explanation serving are permitted.
    Generate,
    /// Only deterministic output is permitted for the requested audience.
    Deterministic,
    /// No explanation output is permitted.
    Refused,
}

/// Decide the artifact-level path. Each document needs its own check.
/// Recheck authority immediately before disclosure and at serving time.
pub fn generation_mode(
    model: &Request,
    model_permissions: &Permissions,
    serving: &Request,
    serving_permissions: &Permissions,
    now_ms: i64,
) -> Mode {
    if model.anchor != serving.anchor
        || model.purpose != Purpose::ModelDisclosure
        || serving.purpose != Purpose::ExplanationServing
        || model.document.is_some()
        || serving.document.is_some()
        || !serving_permissions.allows(serving, now_ms)
    {
        Mode::Refused
    } else if model_permissions.allows(model, now_ms) {
        Mode::Generate
    } else {
        Mode::Deterministic
    }
}

/// Payload-safe refusal at the document boundary.
#[derive(Debug)]
pub struct Error;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("explanation document refused")
    }
}
impl std::error::Error for Error {}

/// Inert context checked for one operation. This is a snapshot of a check,
/// with no market-fact, tool, or retrieval capability.
pub struct AuthorizedExcerpt {
    excerpt: DocumentExcerpt,
    digest: String,
}

impl AuthorizedExcerpt {
    /// Check current permissions, reviewed identity and point-in-time availability.
    ///
    /// # Errors
    /// Rejects malformed, unreviewed, foreign, future or unauthorized excerpts.
    pub fn decode(
        bytes: &[u8],
        request: &Request,
        permissions: &Permissions,
        cutoff_ms: i64,
        now_ms: i64,
    ) -> Result<Self, Error> {
        if !matches!(
            request.purpose,
            Purpose::ResearchUse | Purpose::ModelDisclosure
        ) || cutoff_ms > now_ms
            || !permissions.allows(request, now_ms)
        {
            return Err(Error);
        }
        let excerpt = DocumentExcerpt::decode(bytes).map_err(|_| Error)?;
        let digest = excerpt.id().map_err(|_| Error)?;
        if request.document.as_deref() != Some(digest.as_str())
            || excerpt.scope != request.anchor.scope
            || excerpt.available_at_ms > cutoff_ms
        {
            return Err(Error);
        }
        Ok(Self { excerpt, digest })
    }

    /// Opaque source label.
    pub fn source(&self) -> &str {
        &self.excerpt.source
    }

    /// Reviewed canonical identity.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Untrusted text for a data field. Never interpolate it as instructions.
    pub fn text(&self) -> &str {
        &self.excerpt.text
    }
}
