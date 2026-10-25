use super::*;
use std::num::NonZeroU64;

/// Existing account uncertainty; an opaque block cannot be cleared by preflight.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum QualificationAccountState {
    /// Owner has not established exclusive allocation and remote quiescence.
    Unconfirmed,
    /// A provider operation is unresolved, including another command's call.
    Pending,
    /// Retired unresolved work remains fenced by this exact block generation.
    Blocked(NonZeroU64),
    /// Fresh trusted account read establishes quiescence; not a reservation.
    Quiescent,
}
/// Required independently reviewed controls; absence never means approved.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QualificationControl {
    /// Vendor principal is verified data-only, without any trading capability.
    DataOnlyCredentialScope,
    /// Full calendars, mappings, buckets, actions and schedule provenance are authenticated.
    ReferenceProvenance,
    /// Actual worker/provider/write fencing, not merely expired leases or timeouts.
    RuntimeFencing,
    /// Scoped storage, revocation, erasure, writer quiescence and licensed-copy cleanup.
    RetentionAndCleanup,
    /// Restore denial reconciliation and every consumer's accepted-format compatibility.
    RestoreAndConsumers,
}
/// An owner-authenticated control review bound by the enclosing exact-profile assessment.
/// Not deserializable: untrusted files must never be promoted into trusted assessments.
#[derive(Clone, Eq, PartialEq)]
pub struct QualificationControlReview {
    /// Which specific control the owner verified.
    pub control: QualificationControl,
    /// Opaque independent evidence/review identity, not its contents.
    pub evidence: EvidenceVersion,
    /// Inclusive known/verified time.
    pub known_at: UtcInstant,
    /// Exclusive reviewed validity; must cover original licensed retention.
    pub expires_at: UtcInstant,
}
impl fmt::Debug for QualificationControlReview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QualificationControlReview")
            .field("control", &self.control)
            .finish_non_exhaustive()
    }
}
/// Fresh independently authenticated owner assessment, not transport data or a capability.
#[derive(Clone)]
pub struct QualificationAssessment {
    /// Exact independently approved profile digest; no approval from a hash alone.
    pub profile_sha256: EvidenceDigest,
    /// Current owner opt-in revision; withdrawn/unknown approval must return None/Denied.
    pub owner_review: EvidenceVersion,
    /// Exact trusted clock sample for this assessment; cached samples are rejected.
    pub checked_at: UtcInstant,
    /// Approval/control lifecycle validity, covering the original retention deadline.
    pub expires_at: UtcInstant,
    /// Account independently resolved from the approved data-only principal.
    pub account: ProviderAccountId,
    /// Actual installed policy from the existing durable ledger, not a new limiter.
    pub credits: DailyCreditPolicy,
    /// Actual persisted usage; reads never advance/reset its UTC day.
    pub usage: DailyCreditUsage,
    /// Fresh original command budget/cancellation snapshot; terminal commands must be denied.
    pub budget: BudgetSnapshot,
    /// Fresh account-wide block/pending state, including other commands.
    pub account_state: QualificationAccountState,
    /// Exactly one current authenticated review per required control, at most five.
    pub controls: Vec<QualificationControlReview>,
}
impl fmt::Debug for QualificationAssessment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("QualificationAssessment(..)")
    }
}
impl QualificationAssessment {
    pub(super) fn validate_controls(
        &self,
        now: UtcInstant,
        delete_ms: i64,
    ) -> Result<(), BudgetError> {
        use QualificationControl::*;
        let expected = [
            DataOnlyCredentialScope,
            ReferenceProvenance,
            RuntimeFencing,
            RetentionAndCleanup,
            RestoreAndConsumers,
        ];
        if self.controls.len() != expected.len() {
            return Err(error(K::Denied));
        }
        let mut seen = std::collections::BTreeSet::new();
        for review in &self.controls {
            if !seen.insert(review.control)
                || review.known_at > now
                || review.expires_at.unix_millis() < delete_ms
                || review.expires_at <= now
            {
                return Err(error(K::Denied));
            }
        }
        if expected.iter().any(|c| !seen.contains(c)) {
            return Err(error(K::Denied));
        }
        Ok(())
    }
}
/// Trusted owner adapter, outside request/profile data. No default approval implementation exists.
/// Authenticate operator opt-in/reviews, retain withdrawals, resolve installed policy/usage and
/// command/account state, and independently verify every control for this exact scope/profile.
/// Assessment performs no credential lookup, external call or state mutation. PR15B must supply
/// this boundary and dispatch-time atomic fences; these types cannot prove legal/physical facts.
pub trait QualificationAuthority: Send + Sync {
    /// None means no independently authenticated owner approval; errors preserve denial/uncertainty.
    /// Never echo an untrusted profile into an approval or use an old preflight as fresh authority.
    fn assess(
        &self,
        profile: &QualificationProfile,
        record: &AdmissionRecord,
        now: UtcInstant,
    ) -> Result<Option<QualificationAssessment>, BudgetError>;
}
