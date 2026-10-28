//! Invented reviewed inputs; no credentials, venue-calendar claim or provider connection.
#[allow(dead_code)]
#[path = "../../../../crates/market-data-application/tests/support/streaming.rs"]
mod base;
use chrono::Datelike;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data::offline_streaming::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_http::*;
use edgeagent_market_data_object_store::ImmutableEvidenceStore;
use edgeagent_market_data_postgres::PostgresStreamingLedger as Ledger;
use std::{
    collections::VecDeque,
    num::{NonZeroU16, NonZeroU32, NonZeroU64},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub fn at(ms: i64) -> Result<UtcInstant> {
    Ok(UtcInstant::from_unix_millis(ms)?)
}
fn n(v: u16) -> Result<NonZeroU16> {
    NonZeroU16::new(v).ok_or("bound".into())
}
fn ms(v: u32) -> Result<NonZeroU32> {
    NonZeroU32::new(v).ok_or("bound".into())
}
pub struct Clock(std::sync::Mutex<UtcInstant>);
impl Clock {
    pub fn new(ms: i64) -> Result<Self> {
        Ok(Self(std::sync::Mutex::new(at(ms)?)))
    }
    pub fn set(&self, ms: i64) -> Result {
        *self.0.lock().map_err(|_| "clock")? = at(ms)?;
        Ok(())
    }
}
impl OfflineStreamClock for Clock {
    fn now(&self) -> UtcInstant {
        match self.0.lock() {
            Ok(v) => *v,
            Err(p) => *p.into_inner(),
        }
    }
}
pub struct Fixture {
    date: SessionDate,
    pub base: base::Fixture,
    pub profile: IexCanonicalProfile,
    pub session: StreamingSession,
    pub inputs: StreamOwnerInputs,
    pub mappings: ReferenceHistory<InstrumentMapping>,
    pub calendars: ReferenceHistory<CalendarDay>,
    pub id: u128,
}
impl Fixture {
    pub fn new(ms: i64, id: u128) -> Result<Self> {
        Self::members(ms, id, 1)
    }
    pub fn members(ms: i64, id: u128, members: u32) -> Result<Self> {
        let contract = EvidenceContract::new(
            EvidenceKind::ShareTrade,
            MarketScope::Venue(Venue::Xiex),
            SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
            TimestampPrecision::Nanoseconds,
        )?;
        let day = chrono::DateTime::from_timestamp_millis(ms)
            .ok_or("fixture timestamp")?
            .date_naive();
        let session_date = |d: chrono::NaiveDate| SessionDate::new(d.year(), d.month(), d.day());
        let date = session_date(day)?;
        let dates = DateRange::new(
            date,
            session_date(day.succ_opt().ok_or("fixture next day")?)?,
        )?;
        let open = day
            .and_hms_opt(0, 0, 0)
            .ok_or("fixture midnight")?
            .and_utc()
            .timestamp_millis();
        let mut base = base::Fixture::profile_on(
            ms,
            members,
            contract,
            DeliveryCapability::Snapshots,
            date,
            dates,
        )?;
        let account = ProviderAccountId::new(id)?;
        for s in &mut base.scopes {
            s.account = account;
        }
        base.rights = ReferenceHistory::new(
            base.rights
                .at(at(ms)?)
                .map(|old| {
                    let mut value = *old.value();
                    value.scope.account = account;
                    ReferenceVersion::new(
                        old.id(),
                        old.revision(),
                        old.source(),
                        old.known_at(),
                        *old.checksum(),
                        value,
                    )
                })
                .collect(),
        )?;
        base.providers = ReferenceHistory::new(
            base.providers
                .at(at(ms)?)
                .map(|old| {
                    let mut value = *old.value();
                    value.scope.account = account;
                    ReferenceVersion::new(
                        old.id(),
                        old.revision(),
                        old.source(),
                        old.known_at(),
                        *old.checksum(),
                        value,
                    )
                })
                .collect(),
        )?;
        base.remap_feed(FeedId::new(id)?)?;
        let mut data = base.data(id)?;
        data.delivery = DeliveryCapability::Snapshots;
        let initial = base.seal(data)?;
        let selection = base.plan()?;
        let mappings = selection
            .streaming_routes(&initial)?
            .into_iter()
            .map(|r| r.mapping().clone())
            .collect::<Vec<_>>();
        let calendar = ReferenceVersion::new(
            ReferenceId::new(900)?,
            ReferenceRevision::new(1)?,
            SourceId::new(1)?,
            at(ms - 1)?,
            [9; 32],
            CalendarDay::new(
                Venue::Xnas,
                date,
                // Invented same-day hours, independent of the arrival's time of day.
                SessionHours::Regular(SessionWindow::new(at(open)?, at(open + 86_399_999)?)?),
                "invented-v1",
            )?,
        );
        let lanes = mappings
            .iter()
            .map(|mapping| -> Result<IexIdentityLane> {
                Ok(IexIdentityLane {
                    mapping: mapping.clone(),
                    calendar: calendar.clone(),
                    window: EvidenceInterval::new(
                        PreciseInstant::from_millis(at(ms)?),
                        PreciseInstant::from_millis(at(ms + 3_600_000)?),
                    )?,
                    namespace: NonZeroU64::new(
                        u64::try_from(mapping.value().identities().0.get())? + 6,
                    )
                    .ok_or("namespace")?,
                    scope: IexIdentityScope::Trade {
                        exchange: "V".into(),
                        tape: "C".into(),
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let profile = IexCanonicalProfile::new(
            SourceId::new(1)?,
            contract,
            TransformationRevision::new(1)?,
            &lanes,
            &[IexConditionRule::new(&["@"], &[7])?],
        )?;
        let mut data = base.data(id)?;
        data.delivery = DeliveryCapability::Snapshots;
        data.configuration = profile.digest().clone();
        let session = base.seal(data)?;
        let inputs = profile.owner_inputs(&session, &selection, at(ms + 600_000)?)?;
        Ok(Self {
            date,
            base,
            profile,
            session,
            inputs,
            mappings: ReferenceHistory::new(mappings)?,
            calendars: ReferenceHistory::new(vec![calendar])?,
            id,
        })
    }
    pub fn source<'a>(
        &'a self,
        selection: &'a SelectionPlan<'a>,
        queue: u16,
    ) -> Result<OfflineStreamSource<'a>> {
        Ok(OfflineStreamSource {
            session: &self.session,
            profile: &self.profile,
            selection,
            catalogs: self.base.catalogs(),
            mappings: &self.mappings,
            calendars: &self.calendars,
            inputs: &self.inputs,
            scope: FeedScope::new(
                FeedId::new(self.id)?,
                ProviderCapability::new(
                    SourceId::new(1)?,
                    self.base.contract,
                    DeliveryCapability::Snapshots,
                    KnowledgeCapability::CaptureOnly,
                ),
                TransformationRevision::new(1)?,
                self.date,
            ),
            epoch: FeedEpoch::new(1)?,
            continuity: ContinuityPolicy::new(
                SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
                32,
                2,
                Duration::from_secs(600),
                Duration::from_secs(5),
            )?,
            queue: n(queue)?,
            segment_limits: StreamSegmentLimits {
                bytes: 65536,
                records: 32,
            },
            native_limits: StreamSegmentLimits {
                bytes: 65536,
                records: 32,
            },
            delete_by: at(self.base.ms + 600_000)?,
        })
    }
    pub async fn install(&self, owner: &Ledger) -> Result {
        owner
            .configure_account(self.session.account(), self.base.policy(1, 8)?, 0)
            .await?;
        owner.admit(&self.session, self.base.catalogs()).await?;
        owner
            .install_stream_inputs(&self.session, &self.inputs, self.base.catalogs())
            .await?;
        Ok(())
    }
    pub fn trade(&self, frame: u64, item: u32, id: u64, price: i128) -> Result<IexObservation> {
        self.trade_for(1, frame, item, id, price)
    }
    pub fn trade_for(
        &self,
        instrument: u128,
        frame: u64,
        item: u32,
        id: u64,
        price: i128,
    ) -> Result<IexObservation> {
        Ok(IexObservation {
            mapping: self
                .mappings
                .at(at(self.base.ms)?)
                .find(|m| m.value().identities().0.get() == instrument)
                .ok_or("mapping")?
                .clone(),
            contract: self.base.contract,
            event_at: PreciseInstant::from_millis(at(self.base.ms + 1)?),
            received_at: at(self.base.ms + i64::try_from(frame)?)?,
            frames: [frame, frame],
            item,
            value: IexValue::Trade {
                id,
                value: ShareTrade::new(UsdPrice::new(price, 2)?, ShareVolume::new(7, 0)?)?,
                exchange: "V".into(),
                tape: "C".into(),
                conditions: vec!["@".into()],
            },
        })
    }
    pub fn candidate(&self, id: u64) -> Result<OfflineStreamCandidate> {
        Ok(OfflineStreamCandidate {
            feed: FeedId::new(self.id)?,
            id: ObservationId::new((7u128 << 64) | u128::from(id))?,
            revision: ObservationRevision::new(1)?,
        })
    }
    pub fn policy(&self, requirement: ContinuityRequirement) -> Result<ReadinessPolicy> {
        Ok(ReadinessPolicy::new(
            SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
            Duration::from_secs(1),
            Duration::ZERO,
            &[EvidenceRequirement::new(
                FeedId::new(self.id)?,
                InstrumentId::new(1)?,
                self.base.contract,
                requirement,
            )],
        )?)
    }
}
pub fn limits(inflight: u16) -> Result<OfflineStreamLimits> {
    Ok(OfflineStreamLimits {
        run_ms: ms(15000)?,
        io_ms: ms(5000)?,
        inflight: n(inflight)?,
        queue: n(32)?,
        frames: n(16)?,
        records: n(32)?,
        history: n(16)?,
        history_bytes: ms(1_048_576)?,
    })
}
pub fn frame(ordinal: u64, records: Vec<IexObservation>) -> OfflineStreamFrame {
    OfflineStreamFrame {
        arrival: StreamingArrival { ordinal, bytes: 1 },
        activity: true,
        records,
    }
}
pub fn script(frames: Vec<OfflineStreamFrame>) -> VecDeque<OfflineStreamFrame> {
    frames.into()
}
pub async fn approve<S: StreamSegmentStorage, C: OfflineStreamClock>(
    owner: &Ledger,
    runtime: &OfflineStreamRuntime<'_, S, C>,
    fixtures: &[&Fixture],
) -> Result {
    for (s, witness) in runtime.restores() {
        let f = fixtures
            .iter()
            .find(|f| f.session.data().id == s.data().id)
            .ok_or("fixture")?;
        owner
            .approve_stream_restore(s, witness, f.base.catalogs())
            .await?;
    }
    Ok(())
}
pub struct Store {
    pub inner: ImmutableEvidenceStore,
    pub pause: Option<EvidenceId>,
    pub paused: AtomicBool,
    pub entered: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
    pub fail: AtomicBool,
    pub puts: AtomicUsize,
    pub gets: AtomicUsize,
}
impl Store {
    pub fn new(pause: Option<EvidenceId>) -> Result<Self> {
        Ok(Self {
            inner: ImmutableEvidenceStore::new(
                Arc::new(object_store::memory::InMemory::new()),
                Duration::from_secs(2),
            )?,
            pause,
            paused: AtomicBool::new(true),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            fail: AtomicBool::new(false),
            puts: AtomicUsize::new(0),
            gets: AtomicUsize::new(0),
        })
    }
}
impl StreamSegmentStorage for Store {
    async fn put_segment(
        &self,
        s: &StreamSegment,
        a: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamSegmentObject, EvidenceStorageError> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        let object = self.inner.put_segment(s, a).await?;
        if self.pause.as_ref() == Some(&s.data().scope.session)
            && self.paused.swap(false, Ordering::SeqCst)
        {
            self.entered.notify_one();
            self.release.notified().await;
        }
        Ok(object)
    }
    async fn get_segment(
        &self,
        o: &StreamSegmentObject,
        a: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamSegment, EvidenceStorageError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.inner.get_segment(o, a).await
    }
    async fn put_companion(
        &self,
        c: &StreamCompanion,
        a: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamCompanionObject, EvidenceStorageError> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        let object = self.inner.put_companion(c, a).await?;
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(EvidenceStorageError::new(
                EvidenceStorageErrorKind::Deadline,
            ));
        }
        Ok(object)
    }
    async fn get_companion(
        &self,
        o: &StreamCompanionObject,
        a: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamCompanion, EvidenceStorageError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.inner.get_companion(o, a).await
    }
}
