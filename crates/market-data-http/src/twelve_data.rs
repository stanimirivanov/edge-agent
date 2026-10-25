//! Twelve Data daily protocol. Installing this adapter does not activate a vendor or grant rights.
mod response;
use crate::*;
use edgeagent_contracts::evidence::DateOnlyDailyArtifact;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::{UtcInstant, Venue};
use std::num::NonZeroU32;

/// Explicit trusted native daily binding. No endpoint override, default credential or live mode.
pub struct TwelveDataConfig<'a> {
    /// Complete selected, reviewed native-date schedule, not provider-declared calendars.
    pub schedule: DateOnlySchedule,
    /// Required data-only credential; sensitive Authorization header only.
    pub credential: &'a str,
    /// Explicit polling bound, at most 100 ms.
    pub cancellation_poll_ms: NonZeroU32,
}
/// Immutable single-instrument raw daily request, reusable only under matching durable authority.
pub struct TwelveDataDaily {
    transport: HttpTransport,
    schedule: DateOnlySchedule,
}
/// Single-owner acquisition attempt; retains received bytes across accounting confirmation loss.
pub struct TwelveDataSession<'a, L, A> {
    session: MeteredSession<'a, L, A>,
    adapter: &'a TwelveDataDaily,
    record: AdmissionRecord,
    authority: &'a A,
    started: bool,
}
macro_rules! redact { ($($t:ty),*) => {$(impl std::fmt::Debug for $t {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(concat!(stringify!($t),"(..)"))}
})*}; }
redact!(TwelveDataConfig<'_>, TwelveDataDaily);
impl<L, A> std::fmt::Debug for TwelveDataSession<'_, L, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TwelveDataSession(..)")
    }
}
fn denied() -> DispatchError {
    BudgetError::new(BudgetErrorKind::Denied).into()
}
fn invalid() -> DispatchError {
    HttpError::new(HttpErrorKind::Provider(ProviderFailure::InvalidResponse)).into()
}
fn date(d: edgeagent_market_data_domain::SessionDate) -> String {
    let (y, m, d) = d.ymd();
    format!("{y:04}-{m:02}-{d:02}")
}
fn mic(venue: Venue) -> Result<&'static str, DispatchError> {
    match venue {
        Venue::Xnys => Ok("XNYS"),
        Venue::Xnas => Ok("XNAS"),
        Venue::Arcx => Ok("ARCX"),
        _ => Err(denied()),
    }
}
impl TwelveDataDaily {
    /// Build the fixed HTTPS `/time_series` request. This performs no provider request.
    /// # Errors
    /// Rejects absent/invalid credentials, unsupported binding or transport configuration.
    pub fn new(config: TwelveDataConfig<'_>) -> Result<Self, DispatchError> {
        Self::build(config, "https://api.twelvedata.com/time_series", false)
    }
    #[cfg(test)]
    pub(crate) fn for_loopback(
        config: TwelveDataConfig<'_>,
        endpoint: &str,
    ) -> Result<Self, DispatchError> {
        Self::build(config, endpoint, true)
    }
    fn build(
        config: TwelveDataConfig<'_>,
        endpoint: &str,
        loopback: bool,
    ) -> Result<Self, DispatchError> {
        let r = config.schedule.request();
        let mapping = config.schedule.mapping().value();
        let (symbol, venue, _) = mapping.binding();
        if config.credential.is_empty() {
            return Err(denied());
        }
        let mut url = url::Url::parse(endpoint).map_err(|_| denied())?;
        url.set_query(None);
        // Provider examples exclude a date-only end boundary. End inside the final
        // expected local day, after any native date label, to avoid inclusive/exclusive
        // midnight ambiguity. This is a query bound, never an observation timestamp.
        let end = config.schedule.dates().last().ok_or_else(denied)?;
        let end = format!("{}T23:59:59", date(end));
        url.query_pairs_mut().extend_pairs([
            ("symbol", symbol.as_str()),
            ("mic_code", mic(venue)?),
            ("interval", "1day"),
            ("adjust", "none"),
            ("order", "asc"),
            ("format", "JSON"),
            ("outputsize", "366"),
            ("start_date", &date(r.range.bounds().0)),
            ("end_date", &end),
        ]);
        let transport = HttpTransport::api_key(
            HttpConfig {
                scope: r.scope,
                endpoint: url.as_str(),
                cursor_key: None,
                bearer: Some(config.credential),
                connect_ms: r.budget.connect_ms,
                cancellation_poll_ms: config.cancellation_poll_ms,
            },
            loopback,
        )?;
        Ok(Self {
            transport,
            schedule: config.schedule,
        })
    }
    /// Bind a fresh durable attempt before dispatch; no automatic retry or pagination.
    /// # Errors
    /// Denies different request/listing/configuration, revoked authority or immature evidence.
    pub async fn start<'a, L: AcquisitionLedger, A: DispatchAuthority>(
        &'a self,
        ledger: &'a L,
        record: &AdmissionRecord,
        authority: &'a A,
        cancel: &Cancellation,
    ) -> Result<TwelveDataSession<'a, L, A>, DispatchError> {
        let check = authority.check(record, cancel)?;
        if !check.matches(record)
            || record.request() != self.schedule.request()
            || record.configuration() != Some(self.schedule.configuration_digest().as_str())
            || record.listing()? != self.schedule.mapping().value().identities().1
            || !self
                .schedule
                .satisfies_embargo(check.at(), check.embargo_ms())
        {
            return Err(denied());
        }
        let session = self
            .transport
            .start_metered(ledger, record, authority, cancel)
            .await?;
        Ok(TwelveDataSession {
            session,
            adapter: self,
            record: record.clone(),
            authority,
            started: false,
        })
    }
}
impl<L: AcquisitionLedger, A: DispatchAuthority> TwelveDataSession<'_, L, A> {
    /// Fetch one complete raw daily response. A second fetch is always denied, even after failure.
    /// # Errors
    /// Returns redacted protocol/authority/accounting failures; unknown calls remain charged.
    pub async fn fetch(
        &mut self,
        cancel: &Cancellation,
    ) -> Result<DateOnlyDailyArtifact, DispatchError> {
        if self.started {
            return Err(BudgetError::new(BudgetErrorKind::Busy).into());
        }
        self.started = true;
        let document = self
            .session
            .get_page(&self.record.command().request, None, cancel)
            .await?;
        self.capture(&document, cancel)
    }
    /// Retry only accounting of a known received page; never another provider request.
    /// # Errors
    /// Denies unknown work, cancelled/expired/changed authority or invalid retained payload.
    pub async fn confirm(
        &mut self,
        cancel: &Cancellation,
    ) -> Result<DateOnlyDailyArtifact, DispatchError> {
        let document = self.session.confirm_page(cancel).await?;
        self.capture(&document, cancel)
    }
    fn capture(
        &self,
        document: &JsonDocument,
        cancel: &Cancellation,
    ) -> Result<DateOnlyDailyArtifact, DispatchError> {
        let rows = response::parse(document, &self.adapter.schedule)?;
        let check = self.authority.check(&self.record, cancel)?;
        if !check.matches(&self.record)
            || !self
                .adapter
                .schedule
                .satisfies_embargo(check.at(), check.embargo_ms())
        {
            return Err(denied());
        }
        let received = document.received_at().ok_or_else(invalid)?;
        let derived: UtcInstant = check.at().max(document.derived_at()?);
        if derived.unix_millis() >= check.valid_until_ms() {
            return Err(denied());
        }
        self.adapter
            .schedule
            .capture(
                &self.record,
                &rows,
                edgeagent_contracts::evidence::DailyEvidenceArtifact::hash(document.bytes()),
                received,
                derived,
            )
            .map_err(|_| invalid())
    }
    /// Confirmed durable charges plus local transport observations, without identifiers.
    /// # Errors
    /// Propagates ledger failures; does not substitute local counters for committed usage.
    pub async fn usage(&self) -> Result<DispatchUsage, DispatchError> {
        self.session.usage().await
    }
}
