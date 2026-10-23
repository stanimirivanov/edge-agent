//! Current local authority and identifier-free accounting views for outbound dispatch.
use crate::*;
use edgeagent_market_data_domain::UtcInstant;

/// Trusted owner supplies a fresh authenticated catalog/time assessment, without I/O.
/// A retained token or an external caller's clock cannot implement this trust boundary.
pub trait DispatchAuthority: Send + Sync {
    /// Assess the exact immutable admission against current local authority.
    /// # Errors
    /// Deny missing, changed or expired authority and cancellation; never perform network I/O.
    fn check(
        &self,
        record: &AdmissionRecord,
        cancellation: &Cancellation,
    ) -> Result<DispatchCheck, BudgetError>;
}
/// Sealed current assessment. It grants no independent dispatch or retained-read capability.
pub struct DispatchCheck {
    command: edgeagent_contracts::evidence::AcquireEvidence,
    at: UtcInstant,
    until: i64,
}
impl std::fmt::Debug for DispatchCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DispatchCheck(..)")
    }
}
impl DispatchCheck {
    /// Recheck catalogs, pinned versions, original deadlines and retention at trusted owner time.
    /// # Errors
    /// Rejects cancellation, changed authority or expired original admission.
    pub fn new(
        record: &AdmissionRecord,
        catalogs: AuthorityCatalogs<'_>,
        now: UtcInstant,
        cancellation: &Cancellation,
    ) -> Result<Self, BudgetError> {
        let checked = catalogs
            .admit(record.request(), now, cancellation)
            .map_err(|cause| {
                let kind = match cause {
                    AcquisitionError::Cancelled => BudgetErrorKind::Cancelled,
                    AcquisitionError::Deadline => BudgetErrorKind::Deadline,
                    _ => BudgetErrorKind::Denied,
                };
                BudgetError::with_source(kind, cause)
            })?;
        record.verify_authority(&checked)?;
        let until = record
            .valid_until_ms()?
            .min(checked.delete_by().unix_millis());
        if now.unix_millis() >= until {
            return Err(BudgetError::new(BudgetErrorKind::Deadline));
        }
        Ok(Self {
            command: record.command().clone(),
            at: now,
            until,
        })
    }
    /// Check the assessment belongs to this exact immutable admission.
    #[must_use]
    pub fn matches(&self, record: &AdmissionRecord) -> bool {
        self.command == *record.command()
    }
    /// Trusted UTC assessment sample, never a provider Date header.
    #[must_use]
    pub const fn at(&self) -> UtcInstant {
        self.at
    }
    /// Original exclusive deadline/retention bound; later checks cannot extend it.
    #[must_use]
    pub const fn valid_until_ms(&self) -> i64 {
        self.until
    }
}

/// Confirmed point-in-time ledger counters with no command, account or permit identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcquisitionUsage {
    /// Persisted acquisition attempts.
    pub attempts: u8,
    /// Persisted external starts, including errors and uncertainty.
    pub calls: u32,
    /// Observed bytes plus outstanding/conservative reservations.
    pub charged_bytes: u32,
    /// Whether this command has an unresolved durable call.
    pub pending: bool,
    /// Persisted cancellation; not proof of remote rollback.
    pub cancelled: bool,
}
impl BudgetSnapshot {
    /// Project confirmed counters without exposing opaque command/permit metadata.
    #[must_use]
    pub fn usage(&self) -> AcquisitionUsage {
        AcquisitionUsage {
            attempts: self.attempts,
            calls: self.calls,
            charged_bytes: self.charged_bytes,
            pending: self.pending.is_some(),
            cancelled: self.cancelled,
        }
    }
}
