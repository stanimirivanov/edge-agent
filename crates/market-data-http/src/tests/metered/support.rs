use super::*;
use metrics::{Counter, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicI64, Ordering},
};

pub struct Authority<'a> {
    pub fixture: &'a Fixture,
    pub now: AtomicI64,
    pub deny: AtomicBool,
}
impl DispatchAuthority for Authority<'_> {
    fn check(
        &self,
        record: &AdmissionRecord,
        cancel: &Cancellation,
    ) -> Result<DispatchCheck, BudgetError> {
        if self.deny.load(Ordering::SeqCst) {
            return Err(BudgetError::new(BudgetErrorKind::Denied));
        }
        let now = edgeagent_market_data_domain::UtcInstant::from_unix_millis(
            self.now.load(Ordering::SeqCst),
        )
        .map_err(|_| BudgetError::new(BudgetErrorKind::Invalid))?;
        DispatchCheck::new(record, self.fixture.catalogs(), now, cancel)
    }
}
/// Deterministic committed-state fault model, not a PostgreSQL/process durability proof.
pub struct Ledger {
    pub record: AdmissionRecord,
    state: Mutex<(AcquisitionBudget, AccountBudget)>,
    pub now: AtomicI64,
    pub lose_start: AtomicBool,
    pub pause_start: AtomicBool,
    pub starting: tokio::sync::Notify,
    pub lose_complete: AtomicBool,
    pub pause_complete: AtomicBool,
    pub completing: tokio::sync::Notify,
    pub after_start: Cancellation,
}
impl Ledger {
    pub fn new(f: &Fixture) -> TestResult<Self> {
        use edgeagent_market_data_domain::*;
        let record = AdmissionRecord::new(
            &f.admit(&Cancellation::default())?,
            AcquisitionLimits {
                attempts: 3,
                spacing_ms: 15000,
                starts_per_minute: 4,
            },
            ListingId::new(1)?,
            DailyQualityPolicy::new(TransformationRevision::new(1)?, 60000, 1000, 100)?,
        )?;
        let mut budget = AcquisitionBudget::default();
        budget.begin_attempt(&record, 1, NOW)?;
        let account = AccountBudget::new(record.limits())?;
        Ok(Self {
            record,
            state: Mutex::new((budget, account)),
            now: AtomicI64::new(NOW),
            lose_start: AtomicBool::new(false),
            pause_start: AtomicBool::new(false),
            starting: tokio::sync::Notify::new(),
            lose_complete: AtomicBool::new(false),
            pause_complete: AtomicBool::new(false),
            completing: tokio::sync::Notify::new(),
            after_start: Cancellation::default(),
        })
    }
    pub fn set_time(&self, authority: &Authority<'_>, at: i64) {
        self.now.store(at, Ordering::SeqCst);
        authority.now.store(at, Ordering::SeqCst);
    }
    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, (AcquisitionBudget, AccountBudget)>, BudgetError> {
        self.state
            .lock()
            .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))
    }
}
impl AcquisitionLedger for Ledger {
    async fn admit(
        &self,
        _: &AdmissionRecord,
        _: &AuthorizedAcquisition<'_>,
    ) -> Result<AdmissionRecord, BudgetError> {
        Err(BudgetError::new(BudgetErrorKind::Conflict))
    }
    async fn load(
        &self,
        _: &AcquireEvidence,
    ) -> Result<(AdmissionRecord, BudgetSnapshot), BudgetError> {
        Ok((self.record.clone(), self.lock()?.0.snapshot()))
    }
    async fn begin_attempt(
        &self,
        _: &AcquireEvidence,
        attempt: u8,
    ) -> Result<BudgetSnapshot, BudgetError> {
        let mut s = self.lock()?;
        s.0.begin_attempt(&self.record, attempt, self.now.load(Ordering::SeqCst))?;
        Ok(s.0.snapshot())
    }
    async fn start_call(
        &self,
        _: &AcquireEvidence,
        traversal: Option<&EvidenceId>,
    ) -> Result<CallPermit, BudgetError> {
        let permit = {
            let mut s = self.lock()?;
            let (b, a) = &mut *s;
            b.start(a, &self.record, traversal, self.now.load(Ordering::SeqCst))?
        };
        self.starting.notify_one();
        if self.pause_start.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        self.after_start.cancel();
        if self.lose_start.swap(false, Ordering::SeqCst) {
            return Err(BudgetError::new(BudgetErrorKind::Storage));
        }
        Ok(permit)
    }
    async fn complete_call(
        &self,
        permit: &CallPermit,
        outcome: CallCompletion,
    ) -> Result<BudgetSnapshot, BudgetError> {
        // Persist first, then lose/pause the confirmation, as an uncertain commit reply would.
        let snapshot = {
            let mut s = self.lock()?;
            let (b, a) = &mut *s;
            b.complete(
                a,
                &self.record,
                permit,
                outcome,
                self.now.load(Ordering::SeqCst),
            )?;
            b.snapshot()
        };
        self.completing.notify_one();
        if self.pause_complete.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        if self.lose_complete.swap(false, Ordering::SeqCst) {
            return Err(BudgetError::new(BudgetErrorKind::Storage));
        }
        Ok(snapshot)
    }
    async fn cancel(&self, _: &AcquireEvidence) -> Result<BudgetSnapshot, BudgetError> {
        let mut s = self.lock()?;
        s.0.cancel();
        Ok(s.0.snapshot())
    }
}
pub fn authority(f: &Fixture) -> Authority<'_> {
    Authority {
        fixture: f,
        now: AtomicI64::new(NOW),
        deny: AtomicBool::new(false),
    }
}
pub fn traversal() -> TestResult<EvidenceId> {
    Ok(EvidenceId::new("00000000000000000000000000000001")?)
}
pub fn budget_kind<T>(result: Result<T, DispatchError>, kind: BudgetErrorKind) -> TestResult {
    match result {
        Err(DispatchError::Budget(e)) => assert_eq!(e.kind(), kind),
        _ => return Err("expected budget rejection".into()),
    }
    Ok(())
}
pub fn fixture() -> TestResult<Fixture> {
    let mut f = Fixture::new(42)?;
    f.request.deadline = at(NOW + 60000)?;
    Ok(f)
}

#[derive(Default)]
pub struct Capture(pub Mutex<Vec<Key>>);
impl Recorder for Capture {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        if let Ok(mut keys) = self.0.lock() {
            keys.push(key.clone());
        }
        Counter::noop()
    }
    fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
        Gauge::noop()
    }
    fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
        Histogram::noop()
    }
}
