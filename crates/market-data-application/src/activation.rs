//! Offline preflight only. No credential, provider, persistence or dispatch capability.
mod profile;
mod review;
use crate::*;
use BudgetErrorKind as K;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_domain::{
    BarCoverage, EvidenceKind, MarketScope, TimestampPrecision, UtcInstant,
};
pub use profile::{
    QualificationProfile, QualificationProfileData, QualificationProfileFormat,
    QualificationProvider,
};
pub use review::{
    QualificationAccountState, QualificationAssessment, QualificationAuthority,
    QualificationControl, QualificationControlReview,
};
use std::fmt;

fn error(kind: K) -> BudgetError {
    BudgetError::new(kind)
}
fn instant(t: EvidenceTime) -> Result<UtcInstant, BudgetError> {
    if t.nanos >= 1_000_000_000 || !t.nanos.is_multiple_of(1_000_000) {
        return Err(error(K::Invalid));
    }
    let ms = i128::from(t.seconds) * 1000 + i128::from(t.nanos / 1_000_000);
    UtcInstant::from_unix_millis(i64::try_from(ms).map_err(|_| error(K::Invalid))?)
        .map_err(|_| error(K::Invalid))
}

/// Sealed point-in-time report, deliberately not serializable or accepted by a dispatch port.
/// A pass is neither live qualification nor a reservation, lease or reusable execution grant.
#[must_use = "Preflight is advisory; dispatch still needs fresh authorization and durable reservation."]
pub struct QualificationPreflight {
    provider: QualificationProvider,
    checked_at: UtcInstant,
    until: UtcInstant,
}
impl fmt::Debug for QualificationPreflight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QualificationPreflight")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}
impl QualificationPreflight {
    /// Bounded report label; never report live qualification from this API.
    #[must_use]
    pub const fn status(&self) -> &'static str {
        "preflight_passed_execution_disabled"
    }
    /// Assessed protocol, not a selected live client.
    #[must_use]
    pub const fn provider(&self) -> QualificationProvider {
        self.provider
    }
    /// Trusted assessment sample, not a provider timestamp or cached readiness grant.
    #[must_use]
    pub const fn checked_at(&self) -> UtcInstant {
        self.checked_at
    }
    /// Exclusive finite run-window bound; dispatch must recheck independently.
    #[must_use]
    pub const fn valid_until(&self) -> UtcInstant {
        self.until
    }
}

/// Assess one explicit profile against current authenticated owner inputs, without side effects.
/// PR15B must implement the owner boundary and recheck under dispatch/account fences;
/// constructing a review DTO, passing a checksum or possessing credentials is not approval.
/// # Errors
/// Invalid/Conflict rejects incompatible input; Denied rejects missing/current authority or
/// controls; Busy preserves unresolved work; Exhausted rejects budget; Deadline/Cancelled stop work.
pub fn preflight_qualification(
    profile: &QualificationProfile,
    record: &AdmissionRecord,
    catalogs: AuthorityCatalogs<'_>,
    now: UtcInstant,
    cancel: &Cancellation,
    owner: &impl QualificationAuthority,
) -> Result<QualificationPreflight, BudgetError> {
    if cancel.is_cancelled() {
        return Err(error(K::Cancelled));
    }
    let data = profile.data();
    profile.validate_admission(record, now)?;
    let until = instant(data.expires_at)?;
    let request = record.request();
    let current = catalogs.admit(request, now, cancel).map_err(|e| {
        let kind = match e {
            AcquisitionError::Deadline => K::Deadline,
            AcquisitionError::Cancelled => K::Cancelled,
            _ => K::Denied,
        };
        BudgetError::with_source(kind, e)
    })?;
    record.verify_authority(&current)?;
    let assessment = owner
        .assess(profile, record, now)?
        .ok_or_else(|| error(K::Denied))?;
    if assessment.profile_sha256 != *profile.digest()
        || assessment.owner_review != data.owner_review
        || assessment.account != request.scope.account
        || assessment.credits != data.credits
    {
        return Err(error(K::Conflict));
    }
    if assessment.checked_at != now
        || assessment.expires_at.unix_millis() < record.delete_by_ms()?
    {
        return Err(error(K::Denied));
    }
    assessment.validate_controls(now, record.delete_by_ms()?)?;
    if assessment.account_state != QualificationAccountState::Quiescent {
        return Err(error(K::Busy));
    }
    if cancel.is_cancelled() {
        return Err(error(K::Cancelled));
    }
    profile.validate_remaining_budget(record, &assessment.budget, assessment.usage, now)?;
    Ok(QualificationPreflight {
        provider: data.provider,
        checked_at: now,
        until,
    })
}

impl QualificationProfile {
    /// Check immutable binding, supported protocol and current run window, without granting authority.
    /// # Errors
    /// Rejects changed admission/configuration, incompatible semantics and expired/future windows.
    pub fn validate_admission(
        &self,
        record: &AdmissionRecord,
        now: UtcInstant,
    ) -> Result<(), BudgetError> {
        let data = self.data();
        if data.admission != *record.command()
            || record.configuration() != Some(data.configuration_sha256.as_str())
        {
            return Err(error(K::Conflict));
        }
        let start = instant(data.not_before)?;
        let until = instant(data.expires_at)?;
        if start < record.request().admitted_at
            || now < start
            || now >= until
            || until.unix_millis() > record.valid_until_ms()?
        {
            return Err(error(K::Deadline));
        }
        let request = record.request();
        let precision = match data.provider {
            QualificationProvider::TwelveDataDaily => TimestampPrecision::DateOnly,
            QualificationProvider::AlpacaHistoricalSipDaily => TimestampPrecision::Nanoseconds,
        };
        if request.contract.shape()
            != (
                EvidenceKind::RawBar(BarCoverage::ProviderDay),
                MarketScope::Consolidated,
            )
            || request.contract.rules().1 != precision
            || request.knowledge != KnowledgeMode::Capture
            || request.raw != RawRetention::Discard
        {
            return Err(error(K::Invalid));
        }
        Ok(())
    }
    /// Check fresh persisted command/credit counters. Never reserves, resets or refunds usage.
    /// # Errors
    /// Rejects pending/cancelled/exhausted work and changed, stale or insufficient daily usage.
    pub fn validate_remaining_budget(
        &self,
        record: &AdmissionRecord,
        budget: &BudgetSnapshot,
        usage: DailyCreditUsage,
        now: UtcInstant,
    ) -> Result<(), BudgetError> {
        self.validate_admission(record, now)?;
        let request = record.request();
        if budget.pending.is_some() {
            return Err(error(K::Busy));
        }
        if budget.cancelled {
            return Err(error(K::Cancelled));
        }
        let calls = request
            .budget
            .calls
            .get()
            .checked_sub(budget.calls)
            .filter(|n| *n > 0)
            .ok_or_else(|| error(K::Exhausted))?;
        if budget.attempts > record.limits().attempts
            || budget.charged_bytes >= request.budget.command_bytes.get()
        {
            return Err(error(K::Exhausted));
        }
        if !self.data().credits.covers_qualification(
            request.scope.endpoint,
            usage,
            now.unix_millis(),
            instant(self.data().expires_at)?.unix_millis(),
            calls,
        ) {
            return Err(error(K::Exhausted));
        }
        Ok(())
    }
}
