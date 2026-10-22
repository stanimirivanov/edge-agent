use crate::{
    AcquisitionError as Error, AcquisitionRequest, AudienceId, AuthorityCatalogs,
    AuthorizedAcquisition, Cancellation, KnowledgeMode, PrincipalId, ProviderAccountId,
    RawRetention, StorageScopeId,
};
use edgeagent_market_data_domain::{
    DeliveryCapability, EffectiveInterval, EvidenceContract, InstrumentId, KnowledgeCapability,
    ProviderCapability, ReferenceId, SourceId, UtcInstant,
};
use std::num::NonZeroU32;

/// Exact rights scope. Endpoints use reviewed catalog IDs, never arbitrary URLs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccessScope {
    /// Authenticated requesting principal.
    pub principal: PrincipalId,
    /// Data-only provider account.
    pub account: ProviderAccountId,
    /// Qualified source/feed identity.
    pub source: SourceId,
    /// One admitted instrument; no universe discovery.
    pub instrument: InstrumentId,
    /// Reviewed endpoint/version reference; pagination cannot change it.
    pub endpoint: ReferenceId,
    /// Intended audience, including redistribution restrictions.
    pub audience: AudienceId,
    /// Permitted encrypted storage/location scope.
    pub storage: StorageScopeId,
}
/// Explicit permissions; unknown is not permission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permission {
    /// Not reviewed or unavailable.
    Unknown,
    /// Explicitly prohibited.
    Denied,
    /// Explicitly reviewed and permitted within the grant scope.
    Allowed,
}
/// Required non-display pipeline permissions, independent from raw retention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsePermissions {
    /// Automated non-display processing.
    pub automated: Permission,
    /// Derived evidence generation.
    pub derived: Permission,
    /// Canonical evidence storage in the named scope.
    pub canonical_storage: Permission,
    /// Use by the named audience under its redistribution restrictions.
    pub audience: Permission,
}
/// Append-only review disposition. Withdrawal must remain in retained history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewStatus {
    /// Reviewed, but still subject to scope/time checks.
    Active,
    /// Withdrawn; cannot fall back to an older active revision.
    Revoked,
}
/// Externally authenticated legal/operational review record; not an agreement or credential.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RightsRecord {
    /// Exact allowed scope.
    pub scope: AccessScope,
    /// Exact feed/trade-eligibility semantics.
    pub contract: EvidenceContract,
    /// Opaque reviewed agreement reference, never legal text.
    pub agreement: ReferenceId,
    /// Inclusive effective start/exclusive expiry.
    pub effective: EffectiveInterval,
    /// Required processing/storage/audience permissions.
    pub permissions: UsePermissions,
    /// Independent raw response storage permission.
    pub raw_storage: Permission,
    /// Licensed canonical lifetime; local 24-hour ceiling may shorten it.
    pub canonical_retention_ms: NonZeroU32,
    /// Licensed raw lifetime when explicitly permitted; absent means no raw storage.
    pub raw_retention_ms: Option<NonZeroU32>,
    /// Exclusive agreement-specific deletion deadline.
    pub delete_by: UtcInstant,
    /// Minimum age of a completed source bucket before use, including delayed-feed terms.
    pub embargo_ms: u32,
    /// Explicit active/withdrawn state.
    pub status: ReviewStatus,
}
/// Reviewed provider conformance; source declarations alone cannot create this authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderReview {
    /// Reviewed endpoint/account/source/instrument/audience/storage boundary.
    pub scope: AccessScope,
    /// Qualified semantics, delivery and historical knowledge support.
    pub capability: ProviderCapability,
    /// Review validity, independent from observation event dates.
    pub effective: EffectiveInterval,
    /// Withdrawal remains a new version, never a deleted review.
    pub status: ReviewStatus,
}
impl<'a> AuthorityCatalogs<'a> {
    /// Validate current rights/capability before external work. Latest revisions are
    /// selected before filtering scope/effective time, so corrections cannot revive old grants.
    /// # Errors
    /// Rejects unknown, conflicting, revoked, expired or mismatched authority and invalid requests.
    pub fn admit(
        self,
        request: &'a AcquisitionRequest,
        now: UtcInstant,
        cancellation: &Cancellation,
    ) -> Result<AuthorizedAcquisition<'a>, Error> {
        request.validate(now, cancellation)?;
        let mut rights = self
            .rights
            .at(now)
            .filter(|v| v.value().scope == request.scope && v.value().contract == request.contract);
        let grant = rights.next().ok_or(Error::Rights)?;
        if rights.next().is_some() {
            return Err(Error::Rights);
        }
        let r = grant.value();
        let p = r.permissions;
        if r.status != ReviewStatus::Active
            || !r.effective.contains(now)
            || now >= r.delete_by
            || [p.automated, p.derived, p.canonical_storage, p.audience]
                .iter()
                .any(|p| *p != Permission::Allowed)
            || (request.raw == RawRetention::Retain
                && (r.raw_storage != Permission::Allowed || r.raw_retention_ms.is_none()))
        {
            return Err(Error::Rights);
        }
        let mut providers = self
            .providers
            .at(now)
            .filter(|v| v.value().scope == request.scope);
        let provider = providers.next().ok_or(Error::Conformance)?;
        if providers.next().is_some() {
            return Err(Error::Conformance);
        }
        let review = provider.value();
        let knowledge = match request.knowledge {
            KnowledgeMode::Capture => KnowledgeCapability::CaptureOnly,
            KnowledgeMode::RetainedAt(_) => KnowledgeCapability::HistoricalVersions,
        };
        if review.status != ReviewStatus::Active
            || !review.effective.contains(now)
            || review.capability.source() != request.scope.source
            || review
                .capability
                .require(request.contract, DeliveryCapability::Snapshots, knowledge)
                .is_err()
        {
            return Err(Error::Conformance);
        }
        let lifetime = r.canonical_retention_ms.get().min(86_400_000);
        // Checked UTC years plus a u32 duration fit i64. Clamp before construction
        // so an earlier licensed deadline stays valid near the maximum UTC year.
        let deadline_ms = (now.unix_millis() + i64::from(lifetime))
            .min(r.delete_by.unix_millis())
            .min(r.effective.bounds().1.unix_millis());
        let delete_by = UtcInstant::from_unix_millis(deadline_ms).map_err(|_| Error::Rights)?;
        Ok(AuthorizedAcquisition {
            request,
            rights: grant,
            provider,
            delete_by,
            checked_at: now,
        })
    }
}
impl AuthorizedAcquisition<'_> {
    /// Raw retention deadline, absent when bytes must be discarded. Never exceeds canonical retention.
    #[must_use]
    pub fn raw_delete_by(&self) -> Option<UtcInstant> {
        if self.request.raw == RawRetention::Discard {
            return None;
        }
        self.rights.value().raw_retention_ms.and_then(|ms| {
            let deadline_ms = (self.checked_at.unix_millis() + i64::from(ms.get()))
                .min(self.delete_by.unix_millis());
            UtcInstant::from_unix_millis(deadline_ms).ok()
        })
    }
}
