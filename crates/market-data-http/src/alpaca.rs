//! Historical SIP daily acquisition only; no brokerage endpoint or runtime activation.
mod response;
use crate::*;
use edgeagent_contracts::evidence::DailyEvidenceArtifact;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use std::{collections::BTreeSet, num::NonZeroU32};

/// Explicit trusted data-only credentials and frozen precise daily inputs.
pub struct AlpacaDailyConfig<'a> {
    /// Complete selected/reviewed input schedule; no inferred buckets, IDs or revisions.
    pub schedule: PreciseDailySchedule,
    /// Sensitive APCA-API-KEY-ID header; never a query parameter.
    pub key_id: &'a str,
    /// Sensitive APCA-API-SECRET-KEY header; no Basic/Broker API mode.
    pub secret: &'a str,
    /// Explicit cancellation polling ceiling, at most 100 ms.
    pub cancellation_poll_ms: NonZeroU32,
}
/// Fixed historical market-data endpoint; no feed fallback or mutable alias remapping.
pub struct AlpacaDaily {
    transport: HttpTransport,
    schedule: PreciseDailySchedule,
    url: url::Url,
}
/// Result of one explicitly requested page, never an automatically driven traversal.
#[derive(Debug)]
pub enum AlpacaProgress {
    /// More pages exist; caller must honor durable spacing/budgets before requesting one.
    More,
    /// Complete canonical inputs, still requiring authority/quality acceptance.
    Complete(DailySourceCapture),
}
enum State {
    Ready(Option<String>),
    Unconfirmed,
    Finished,
}
/// Single-owner bounded traversal; confirmation retries cannot issue another HTTP request.
pub struct AlpacaSession<'a, L, A> {
    session: MeteredSession<'a, L, A>,
    adapter: &'a AlpacaDaily,
    record: AdmissionRecord,
    authority: &'a A,
    state: State,
    rows: Vec<PreciseDailyRow>,
    receipts: Vec<DailyPageReceipt>,
    seen: BTreeSet<String>,
}
macro_rules! redact { ($($t:ty),*) => {$(impl std::fmt::Debug for $t {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(concat!(stringify!($t),"(..)"))}
})*}; }
redact!(AlpacaDailyConfig<'_>, AlpacaDaily);
impl<L, A> std::fmt::Debug for AlpacaSession<'_, L, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AlpacaSession(..)")
    }
}
fn denied() -> DispatchError {
    BudgetError::new(BudgetErrorKind::Denied).into()
}
fn invalid() -> DispatchError {
    HttpError::new(HttpErrorKind::Provider(ProviderFailure::InvalidResponse)).into()
}
fn timestamp(text: &str) -> Result<PreciseInstant, DispatchError> {
    // Explicit UTC only; reject unknown offsets, leap seconds, dates and fractions
    // beyond nine digits instead of accepting Chrono's permissive truncation.
    if text.len() < 20
        || text.len() > 30
        || !text.ends_with('Z')
        || text.as_bytes().get(10) != Some(&b'T')
    {
        return Err(invalid());
    }
    let at = chrono::DateTime::parse_from_rfc3339(text).map_err(|_| invalid())?;
    PreciseInstant::from_unix_parts(at.timestamp(), at.timestamp_subsec_nanos())
        .map_err(|_| invalid())
}
fn rfc(at: PreciseInstant) -> Result<String, DispatchError> {
    let (seconds, nanos) = at.unix_parts();
    let nanos = u32::try_from(nanos).map_err(|_| invalid())?;
    let time = chrono::DateTime::from_timestamp(seconds, nanos).ok_or_else(invalid)?;
    Ok(time.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
}
impl AlpacaDaily {
    /// Build a fixed HTTPS raw historical SIP request, without performing network I/O.
    /// # Errors
    /// Rejects missing credentials, invalid time bindings or transport configuration.
    pub fn new(config: AlpacaDailyConfig<'_>) -> Result<Self, DispatchError> {
        Self::build(config, "https://data.alpaca.markets/v2/stocks/bars", false)
    }
    #[cfg(test)]
    pub(crate) fn for_loopback(
        config: AlpacaDailyConfig<'_>,
        endpoint: &str,
    ) -> Result<Self, DispatchError> {
        Self::build(config, endpoint, true)
    }
    fn build(
        config: AlpacaDailyConfig<'_>,
        endpoint: &str,
        loopback: bool,
    ) -> Result<Self, DispatchError> {
        let r = config.schedule.request();
        let mut url = url::Url::parse(endpoint).map_err(|_| denied())?;
        url.set_query(None);
        let first = config
            .schedule
            .slots()
            .first()
            .ok_or_else(denied)?
            .bucket
            .bounds()
            .0;
        let last = config
            .schedule
            .slots()
            .last()
            .ok_or_else(denied)?
            .bucket
            .bounds()
            .0;
        url.query_pairs_mut().extend_pairs([
            (
                "symbols",
                config.schedule.mapping().value().binding().0.as_str(),
            ),
            ("timeframe", "1Day"),
            ("feed", "sip"),
            ("adjustment", "raw"),
            ("currency", "USD"),
            ("asof", "-"),
            ("sort", "asc"),
            ("limit", "252"),
            ("start", &rfc(first)?),
            ("end", &rfc(last)?),
        ]);
        let transport = HttpTransport::alpaca(
            HttpConfig {
                scope: r.scope,
                endpoint: url.as_str(),
                cursor_key: Some("page_token"),
                bearer: None,
                connect_ms: r.budget.connect_ms,
                cancellation_poll_ms: config.cancellation_poll_ms,
            },
            config.key_id,
            config.secret,
            loopback,
        )?;
        Ok(Self {
            transport,
            schedule: config.schedule,
            url,
        })
    }
    /// Bind exact durable admission and current authority before any provider call.
    /// # Errors
    /// Denies changed input pins, nonhistorical bounds or incompatible reviewed capability.
    pub async fn start<'a, L: AcquisitionLedger, A: DispatchAuthority>(
        &'a self,
        ledger: &'a L,
        record: &AdmissionRecord,
        authority: &'a A,
        cancel: &Cancellation,
    ) -> Result<AlpacaSession<'a, L, A>, DispatchError> {
        let check = authority.check(record, cancel)?;
        self.check(record, &check)?;
        let session = self
            .transport
            .start_metered(ledger, record, authority, cancel)
            .await?;
        Ok(AlpacaSession {
            session,
            adapter: self,
            record: record.clone(),
            authority,
            state: State::Ready(None),
            rows: Vec::new(),
            receipts: Vec::new(),
            seen: BTreeSet::new(),
        })
    }
    fn check(&self, record: &AdmissionRecord, check: &DispatchCheck) -> Result<(), DispatchError> {
        let r = self.schedule.request();
        if !check.matches(record)
            || record.request() != r
            || record.configuration() != Some(self.schedule.configuration_digest().as_str())
            || record.listing()? != self.schedule.mapping().value().identities().1
            || check.provider()
                != ProviderCapability::new(
                    r.scope.source,
                    r.contract,
                    DeliveryCapability::Snapshots,
                    KnowledgeCapability::CaptureOnly,
                )
            || !self
                .schedule
                .satisfies_embargo(check.at(), check.embargo_ms().max(900_000))
        {
            return Err(denied());
        }
        Ok(())
    }
}
impl<L: AcquisitionLedger, A: DispatchAuthority> AlpacaSession<'_, L, A> {
    /// Request exactly one page. No automatic pagination, retry, wait or IEX fallback occurs.
    /// # Errors
    /// Known pre-dispatch rate rejection leaves the same page ready; uncertain work stays fenced.
    pub async fn fetch_page(
        &mut self,
        cancel: &Cancellation,
    ) -> Result<AlpacaProgress, DispatchError> {
        let State::Ready(token) = &self.state else {
            return Err(BudgetError::new(BudgetErrorKind::Busy).into());
        };
        let token = token.clone();
        let check = self.authority.check(&self.record, cancel)?;
        self.adapter.check(&self.record, &check)?;
        let next = token.as_ref().map(|token| {
            let mut url = self.adapter.url.clone();
            url.query_pairs_mut().append_pair("page_token", token);
            url.to_string()
        });
        self.state = State::Unconfirmed;
        match self
            .session
            .get_page(&self.record.command().request, next.as_deref(), cancel)
            .await
        {
            Ok(document) => self.accept(document, cancel),
            Err(error) => {
                if matches!(&error,DispatchError::Budget(e) if e.kind()==BudgetErrorKind::RateLimited)
                    && self.session.idle()
                {
                    self.state = State::Ready(token);
                }
                Err(error)
            }
        }
    }
    /// Reconcile accounting for one already received page, preserving its original receipt.
    /// # Errors
    /// Unknown/dropped work cannot be refetched here; revoked/expired data is not released.
    pub async fn confirm_page(
        &mut self,
        cancel: &Cancellation,
    ) -> Result<AlpacaProgress, DispatchError> {
        if !matches!(self.state, State::Unconfirmed) {
            return Err(BudgetError::new(BudgetErrorKind::Busy).into());
        }
        let document = self.session.confirm_page(cancel).await?;
        self.accept(document, cancel)
    }
    fn accept(
        &mut self,
        document: JsonDocument,
        cancel: &Cancellation,
    ) -> Result<AlpacaProgress, DispatchError> {
        self.state = State::Finished; // Any schema/authority failure terminates this traversal.
        let page = response::parse(
            &document,
            self.adapter.schedule.mapping().value().binding().0.as_str(),
        )?;
        let check = self.authority.check(&self.record, cancel)?;
        self.adapter.check(&self.record, &check)?;
        if document
            .derived_at()?
            .unix_millis()
            .max(check.at().unix_millis())
            >= check.valid_until_ms()
        {
            return Err(denied());
        }
        let offset = self.rows.len();
        let slots = self.adapter.schedule.slots();
        if page.rows.len() > slots.len().saturating_sub(offset)
            || page
                .rows
                .iter()
                .zip(slots.iter().skip(offset))
                .any(|(r, s)| r.event_at != s.bucket.bounds().0)
        {
            return Err(invalid());
        }
        for (row, slot) in page.rows.iter().zip(slots.iter().skip(offset)) {
            RawBar::new(slot.bucket, BarCoverage::ProviderDay, row.ohlc, row.volume)
                .map_err(|_| invalid())?;
        }
        if let Some(token) = &page.next
            && (!self.seen.insert(token.clone())
                || offset + page.rows.len() >= slots.len()
                || self.receipts.len() + 1 >= self.record.request().budget.pages.get() as usize)
        {
            return Err(invalid());
        }
        self.rows.extend(page.rows);
        self.receipts.push(DailyPageReceipt {
            body_sha256: DailyEvidenceArtifact::hash(document.bytes()),
            received_at: document.received_at().ok_or_else(invalid)?,
        });
        if let Some(token) = page.next {
            self.state = State::Ready(Some(token));
            Ok(AlpacaProgress::More)
        } else {
            self.adapter
                .schedule
                .capture(
                    &self.record,
                    check.provider(),
                    &self.rows,
                    std::mem::take(&mut self.receipts),
                )
                .map(AlpacaProgress::Complete)
                .map_err(|_| invalid())
        }
    }
    /// Confirmed identifier-free durable accounting, distinct from local observations.
    /// # Errors
    /// Propagates storage or immutable-admission conflicts.
    pub async fn usage(&self) -> Result<DispatchUsage, DispatchError> {
        self.session.usage().await
    }
}
