use crate::{
    AcquisitionError as Error, AcquisitionId, AuthorityCatalogs, AuthorizedAcquisition,
    Cancellation, KnowledgeMode,
};
use edgeagent_market_data_domain::{
    CanonicalObservation, PreciseInstant, ProviderCapability, UtcInstant,
};

/// Stable payload-free provider failures; adapters must not attach URLs, payloads or SDK errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProviderFailure {
    /// Retry only within the remaining durable budget; delay is provider-directed milliseconds.
    Transient {
        /// Optional bounded retry delay; not permission to reset a budget.
        retry_after_ms: Option<u32>,
    },
    /// Request timed out; remote execution may be uncertain.
    Timeout,
    /// Cancellation requested; remote execution may already have occurred.
    Cancelled,
    /// Authentication/entitlement rejection; terminal until reviewed configuration changes.
    Forbidden,
    /// Unsupported endpoint, semantics or knowledge capability.
    Unsupported,
    /// Invalid, corrupt or oversized provider data; never silently repair.
    InvalidResponse,
}
impl std::fmt::Display for ProviderFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "provider failure: {self:?}")
    }
}
impl std::error::Error for ProviderFailure {}
/// Application-owned substitution seam. No implicit retries, hedging or hidden pagination.
/// Adapters enforce request byte/time/page bounds while reading, check cancellation,
/// and never hold service database transactions across I/O. Dropping a future does
/// not undo remote effects. Only data acquisition is allowed, never order routing.
pub trait DailyProvider: Send + Sync {
    /// Fetch bounded canonical inputs under the exact admitted authority. This does
    /// not grant quality/publication acceptance; run `validate_batch`, then PR4 quality.
    fn fetch(
        &self,
        request: &AuthorizedAcquisition<'_>,
        cancellation: &Cancellation,
    ) -> impl std::future::Future<Output = Result<ProviderBatch, ProviderFailure>> + Send;
}
/// Untrusted adapter result; bounds apply before allocation/parsing in real adapters too.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderBatch {
    /// Exact immutable acquisition command identity.
    pub request: AcquisitionId,
    /// Actual source declaration; not conformance authority.
    pub provider: ProviderCapability,
    /// Canonical revisions; values are still subject to whole-series quality checks.
    pub observations: Vec<CanonicalObservation>,
}
/// Reusable offline provider conformance boundary. Recheck current rights/review
/// revisions after I/O; any changed authority requires fresh admission. This is
/// not a timeout executor, physical deletion mechanism or latest-session resolver.
/// # Errors
/// Rejects cancelled, expired, changed authority and malformed/oversized/mismatched evidence.
pub fn validate_batch(
    admitted: &AuthorizedAcquisition<'_>,
    catalogs: AuthorityCatalogs<'_>,
    now: UtcInstant,
    cancellation: &Cancellation,
    batch: &ProviderBatch,
) -> Result<(), Error> {
    let current = catalogs.admit(admitted.request, now, cancellation)?;
    if now >= admitted.delete_by {
        return Err(Error::Rights);
    }
    if current.rights != admitted.rights {
        return Err(Error::Rights);
    }
    if current.provider != admitted.provider {
        return Err(Error::Conformance);
    }
    if batch.request != admitted.request.id
        || batch.provider != admitted.provider.value().capability
        || batch.observations.is_empty()
        || batch.observations.len() > 4096
    {
        return Err(Error::Response);
    }
    let cutoff = PreciseInstant::from_millis(match admitted.request.knowledge {
        KnowledgeMode::Capture => now,
        KnowledgeMode::RetainedAt(cutoff) => cutoff,
    });
    for observation in &batch.observations {
        let endpoint = match observation.value() {
            Some(edgeagent_market_data_domain::ObservationValue::Bar(bar)) => {
                bar.bucket().0.bounds().1
            }
            None => observation.times().event_at(),
            _ => return Err(Error::Response),
        };
        let (end_seconds, end_nanos) = endpoint.unix_parts();
        let embargo_age = i128::from(now.unix_millis()) * 1_000_000
            - (i128::from(end_seconds) * 1_000_000_000 + i128::from(end_nanos));
        if embargo_age < i128::from(admitted.rights.value().embargo_ms) * 1_000_000 {
            return Err(Error::Rights);
        }
        let (instrument, provider, _) = observation.context().origin();
        if instrument != admitted.request.instrument()
            || provider != batch.provider
            || !admitted
                .request
                .range
                .contains(observation.context().session_conditions().0)
            || observation.times().received_at() > cutoff
        {
            return Err(Error::Response);
        }
    }
    Ok(())
}
