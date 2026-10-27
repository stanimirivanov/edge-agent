use super::*;
use edgeagent_market_data_domain::InstrumentId;
use std::{
    future::Future,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

fn denied() -> BudgetError {
    BudgetError::new(BudgetErrorKind::Denied)
}
fn boundary() -> StreamCaptureBoundary {
    StreamCaptureBoundary {
        history_start: true,
        history_retired: false,
        reconnect: false,
        skipped: false,
    }
}
fn run<T>(future: impl Future<Output = T>) -> Result<T> {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(value) => Ok(value),
        std::task::Poll::Pending => Err("pure mock unexpectedly yielded".into()),
    }
}
struct Ledger {
    bytes: Vec<u8>,
    pending: AtomicBool,
    committed: AtomicBool,
}
impl StreamPublicationLedger for Ledger {
    async fn pending_segment(
        &self,
        _: &StreamingSession,
        _: InstrumentId,
        _: AuthorityCatalogs<'_>,
    ) -> std::result::Result<Option<StreamPublication>, BudgetError> {
        if self.pending.load(Ordering::SeqCst) {
            Ok(Some(StreamPublication::decode(&self.bytes)?))
        } else {
            Ok(None)
        }
    }
    async fn recover_publication(
        &self,
        _: &StreamingSession,
        p: &StreamPublication,
        _: AuthorityCatalogs<'_>,
    ) -> std::result::Result<Option<StreamCaptureBoundary>, BudgetError> {
        if p.bytes()? != self.bytes {
            return Err(denied());
        }
        Ok(self.committed.load(Ordering::SeqCst).then_some(boundary()))
    }
    async fn stage(
        &self,
        _: &StreamingSession,
        p: &StreamPublication,
        _: AuthorityCatalogs<'_>,
        _: &Cancellation,
    ) -> std::result::Result<(), BudgetError> {
        if p.bytes()? != self.bytes {
            return Err(denied());
        }
        Ok(())
    }
    async fn recover_segment(
        &self,
        _: &StreamingSession,
        _: &EvidenceId,
        _: AuthorityCatalogs<'_>,
    ) -> std::result::Result<Option<StreamPublication>, BudgetError> {
        Ok(Some(StreamPublication::decode(&self.bytes)?))
    }
    async fn commit_segment(
        &self,
        _: &StreamingSession,
        p: &StreamPublication,
        _: AuthorityCatalogs<'_>,
        _: &Cancellation,
    ) -> std::result::Result<StreamCaptureBoundary, BudgetError> {
        if p.bytes()? != self.bytes {
            return Err(denied());
        }
        self.committed.store(true, Ordering::SeqCst);
        self.pending.store(false, Ordering::SeqCst);
        Ok(boundary())
    }
    async fn published_segment(
        &self,
        _: &StreamingSession,
        _: &EvidenceId,
        _: AuthorityCatalogs<'_>,
    ) -> std::result::Result<StreamSegmentObject, BudgetError> {
        Err(denied())
    }
}
struct Barrier {
    checks: AtomicUsize,
    fail_at: usize,
}
impl StreamRecoveryLedger for Barrier {
    async fn check_restoring(
        &self,
        s: &StreamingSession,
        r: &StreamRestore,
        _: AuthorityCatalogs<'_>,
    ) -> std::result::Result<(), BudgetError> {
        if r.identity().0 != &s.data().id
            || self.checks.fetch_add(1, Ordering::SeqCst) >= self.fail_at
        {
            return Err(denied());
        }
        Ok(())
    }
}
struct Store {
    puts: AtomicUsize,
}
impl StreamSegmentStorage for Store {
    async fn put_segment(
        &self,
        s: &StreamSegment,
        a: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamSegmentObject, EvidenceStorageError> {
        a.authorize(&s.object())?;
        self.puts.fetch_add(1, Ordering::SeqCst);
        Ok(s.object())
    }
    async fn get_segment(
        &self,
        _: &StreamSegmentObject,
        _: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamSegment, EvidenceStorageError> {
        Err(EvidenceStorageError::new(
            EvidenceStorageErrorKind::NotFound,
        ))
    }
}
struct Access(edgeagent_market_data_domain::UtcInstant);
impl StreamSegmentAccess for Access {
    fn now(&self) -> edgeagent_market_data_domain::UtcInstant {
        self.0
    }
    fn authorize(&self, _: &StreamSegmentObject) -> std::result::Result<(), EvidenceStorageError> {
        Ok(())
    }
}
#[test]
fn stream_restore_replays_original_intent_and_rechecks_after_effects() -> Result {
    let f = fixture::Fixture::new(fixture::AT, 1)?;
    let s = f.session(1)?;
    let p = publication()?;
    let ledger = Ledger {
        bytes: p.bytes()?,
        pending: AtomicBool::new(true),
        committed: AtomicBool::new(false),
    };
    let store = Store {
        puts: AtomicUsize::new(0),
    };
    let restore = StreamRestore::from_committed(s.data().id.clone(), 1)?;
    let barrier = Barrier {
        checks: AtomicUsize::new(0),
        fail_at: 1,
    };
    let access = Access(fixture::at(fixture::AT + 100)?);
    assert!(
        run(recover_pending_stream_segment(
            &ledger,
            &barrier,
            &store,
            &s,
            &restore,
            InstrumentId::new(1)?,
            f.catalogs(),
            &Cancellation::default(),
            &access
        ))?
        .is_err()
    );
    assert!(ledger.committed.load(Ordering::SeqCst));
    assert!(!ledger.pending.load(Ordering::SeqCst));
    assert_eq!(store.puts.load(Ordering::SeqCst), 1);
    // Exact committed-result recovery never repeats PUT, even after the post-effect denial.
    assert_eq!(
        run(publish_stream_segment(
            &ledger,
            &store,
            &s,
            &p,
            f.catalogs(),
            &Cancellation::default(),
            &access
        ))??,
        boundary()
    );
    assert_eq!(store.puts.load(Ordering::SeqCst), 1);
    assert_eq!(ledger.bytes, p.bytes()?);
    Ok(())
}
#[test]
fn stream_restore_empty_cancelled_or_stale_recovery_never_opens_barrier() -> Result {
    let f = fixture::Fixture::new(fixture::AT, 1)?;
    let s = f.session(1)?;
    let p = publication()?;
    let ledger = Ledger {
        bytes: p.bytes()?,
        pending: AtomicBool::new(false),
        committed: AtomicBool::new(false),
    };
    let store = Store {
        puts: AtomicUsize::new(0),
    };
    let restore = StreamRestore::from_committed(s.data().id.clone(), 1)?;
    let barrier = Barrier {
        checks: AtomicUsize::new(0),
        fail_at: usize::MAX,
    };
    let cancel = Cancellation::default();
    let access = Access(fixture::at(fixture::AT + 100)?);
    assert!(
        run(recover_pending_stream_segment(
            &ledger,
            &barrier,
            &store,
            &s,
            &restore,
            InstrumentId::new(1)?,
            f.catalogs(),
            &cancel,
            &access
        ))??
        .is_none()
    );
    assert_eq!(barrier.checks.load(Ordering::SeqCst), 2);
    cancel.cancel();
    assert!(
        run(recover_pending_stream_segment(
            &ledger,
            &barrier,
            &store,
            &s,
            &restore,
            InstrumentId::new(1)?,
            f.catalogs(),
            &cancel,
            &access
        ))?
        .is_err()
    );
    assert_eq!(barrier.checks.load(Ordering::SeqCst), 2);
    let stale = Barrier {
        checks: AtomicUsize::new(0),
        fail_at: 0,
    };
    assert!(
        run(recover_pending_stream_segment(
            &ledger,
            &stale,
            &store,
            &s,
            &restore,
            InstrumentId::new(1)?,
            f.catalogs(),
            &Cancellation::default(),
            &access
        ))?
        .is_err()
    );
    assert_eq!(store.puts.load(Ordering::SeqCst), 0);
    assert!(StreamRestore::from_committed(s.data().id.clone(), 0).is_err());
    Ok(())
}
