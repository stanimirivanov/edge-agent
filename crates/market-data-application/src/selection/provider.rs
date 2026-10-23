use super::SelectionPlan;
use crate::{AuthorizedAcquisition, Cancellation, DailyProvider, ProviderBatch, ProviderFailure};

/// Consumer-side selection decorator; existing providers and authority/quality ports stay intact.
pub struct SelectedDailyProvider<'a, P> {
    plan: &'a SelectionPlan<'a>,
    inner: &'a P,
}
impl<'a, P> SelectedDailyProvider<'a, P> {
    /// Compose a pinned plan around an existing provider; no provider is invoked here.
    #[must_use]
    pub const fn new(plan: &'a SelectionPlan<'a>, inner: &'a P) -> Self {
        Self { plan, inner }
    }
}
impl<P: DailyProvider> DailyProvider for SelectedDailyProvider<'_, P> {
    async fn fetch(
        &self,
        request: &AuthorizedAcquisition<'_>,
        cancel: &Cancellation,
    ) -> Result<ProviderBatch, ProviderFailure> {
        if cancel.is_cancelled() {
            return Err(ProviderFailure::Cancelled);
        }
        self.plan
            .prepare(request.request())
            .map_err(|_| ProviderFailure::Unsupported)?;
        self.inner.fetch(request, cancel).await
    }
}
impl<P> std::fmt::Debug for SelectedDailyProvider<'_, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SelectedDailyProvider(..)")
    }
}
