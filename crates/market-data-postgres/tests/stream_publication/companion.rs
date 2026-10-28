use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[derive(Default)]
struct PairStore {
    segment: Mutex<Option<StreamSegment>>,
    native: Mutex<Option<StreamCompanion>>,
    fail: AtomicBool,
    puts: AtomicUsize,
}
fn storage() -> EvidenceStorageError {
    EvidenceStorageError::new(EvidenceStorageErrorKind::Backend)
}
impl StreamSegmentStorage for PairStore {
    async fn put_segment(
        &self,
        s: &StreamSegment,
        a: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamSegmentObject, EvidenceStorageError> {
        a.authorize(&s.object())?;
        self.puts.fetch_add(1, Ordering::SeqCst);
        let mut old = self.segment.lock().map_err(|_| storage())?;
        if old.as_ref().is_some_and(|v| v != s) {
            return Err(storage());
        }
        *old = Some(s.clone());
        Ok(s.object())
    }
    async fn get_segment(
        &self,
        o: &StreamSegmentObject,
        a: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamSegment, EvidenceStorageError> {
        a.authorize(o)?;
        self.segment
            .lock()
            .map_err(|_| storage())?
            .clone()
            .ok_or_else(storage)
    }
    async fn put_companion(
        &self,
        c: &StreamCompanion,
        a: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamCompanionObject, EvidenceStorageError> {
        a.authorize(&c.object().segment)?;
        self.puts.fetch_add(1, Ordering::SeqCst);
        let mut old = self.native.lock().map_err(|_| storage())?;
        if old.as_ref().is_some_and(|v| v != c) {
            return Err(storage());
        }
        *old = Some(c.clone());
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(storage());
        }
        Ok(c.object())
    }
    async fn get_companion(
        &self,
        o: &StreamCompanionObject,
        a: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamCompanion, EvidenceStorageError> {
        a.authorize(&o.segment)?;
        self.native
            .lock()
            .map_err(|_| storage())?
            .clone()
            .ok_or_else(storage)
    }
}
impl StreamDeletion for PairStore {
    async fn delete_segment(
        &self,
        c: &StreamCleanup,
        a: &impl StreamDeletionAuthority,
    ) -> std::result::Result<(), EvidenceStorageError> {
        a.confirm_storage_quiescent(c)?;
        *self.native.lock().map_err(|_| storage())? = None;
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(storage());
        }
        *self.segment.lock().map_err(|_| storage())? = None;
        a.confirm_storage_quiescent(c)
    }
}
#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn native_pair_publication_recovers_exact_receipt_and_retains_partial_cleanup_inventory()
-> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let operation = async {
        let f = fixture::Fixture::new(h.now - 100, 1)?;
        let (s, inputs) = super::inputs::record(&f, 1, 7, br#"["invented",1]"#)?;
        let owner = Ledger::new(h.pool.clone());
        let worker = Ledger::new(pool.clone());
        owner
            .configure_account(f.account()?, f.policy(1, 4)?, 0)
            .await?;
        owner.admit(&s, f.catalogs()).await?;
        let connection = worker
            .reserve(&s, f.catalogs(), &Cancellation::default())
            .await?;
        charge(&worker, &f, &s, &connection, 1).await?;
        charge(&worker, &f, &s, &connection, 2).await?;
        let legacy = proposal(&f, &s, &connection, 1, [1, 2], None)?;
        let native = StreamCompanion::new(StreamCompanionData {
            version: StreamCompanionFormat::V1,
            limits: StreamSegmentLimits {
                bytes: 65536,
                records: 16,
            },
            segment: legacy.segment().object(),
            owner_inputs: inputs.bytes().to_vec(),
            records: vec![],
        })?;
        let p = legacy.with_companion(native.clone())?;
        let access = Access {
            object: p.segment().object(),
            at: fixture::at(h.now)?,
        };
        let store = PairStore::default();
        store.fail.store(true, Ordering::SeqCst);
        // A self-consistent profile cannot approve itself or reach any object PUT.
        assert!(
            publish_stream_segment(
                &worker,
                &store,
                &s,
                &p,
                f.catalogs(),
                &Cancellation::default(),
                &access
            )
            .await
            .is_err()
        );
        assert_eq!(store.puts.load(Ordering::SeqCst), 0);
        owner
            .install_stream_inputs(&s, &inputs, f.catalogs())
            .await?;
        assert!(
            publish_stream_segment(
                &worker,
                &store,
                &s,
                &p,
                f.catalogs(),
                &Cancellation::default(),
                &access
            )
            .await
            .is_err()
        );
        let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM edgeagent_stream_publications WHERE published),(SELECT count(*) FROM edgeagent_message_outbox)").fetch_one(&h.pool).await?;
        assert_eq!(counts, (0, 0));
        let recovered = worker
            .pending_segment(&s, InstrumentId::new(1)?, f.catalogs())
            .await?
            .ok_or("pending")?;
        assert_eq!(recovered.bytes()?, p.bytes()?);
        let mut changed = native.data().clone();
        changed.limits.bytes -= 1;
        let changed = proposal(&f, &s, &connection, 1, [1, 2], None)?
            .with_companion(StreamCompanion::new(changed)?)?;
        let before = store.puts.load(Ordering::SeqCst);
        assert!(
            publish_stream_segment(
                &worker,
                &store,
                &s,
                &changed,
                f.catalogs(),
                &Cancellation::default(),
                &access
            )
            .await
            .is_err()
        );
        assert_eq!(store.puts.load(Ordering::SeqCst), before);
        let receipt = publish_stream_segment(
            &worker,
            &store,
            &s,
            &recovered,
            f.catalogs(),
            &Cancellation::default(),
            &access,
        )
        .await?;
        let before = store.puts.load(Ordering::SeqCst);
        assert_eq!(
            publish_stream_segment(
                &worker,
                &store,
                &s,
                &p,
                f.catalogs(),
                &Cancellation::default(),
                &access
            )
            .await?,
            receipt
        );
        assert_eq!(store.puts.load(Ordering::SeqCst), before);
        // A parallel object map cannot replace the receipt's exact native binding.
        sqlx::query("UPDATE edgeagent_stream_publications SET descriptor=$1")
            .bind(serde_json::to_string(&p.segment().object())?)
            .execute(&h.pool)
            .await?;
        assert!(
            worker
                .published_companion(&s, p.id(), f.catalogs())
                .await
                .is_err()
        );
        assert!(
            worker
                .recover_publication(&s, &p, f.catalogs())
                .await
                .is_err()
        );
        sqlx::query("UPDATE edgeagent_stream_publications SET descriptor=$1")
            .bind(p.descriptor()?)
            .execute(&h.pool)
            .await?;
        let (canonical, n) =
            read_stream_publication(&worker, &store, &s, p.id(), f.catalogs(), &access).await?;
        assert_eq!(canonical, *p.segment());
        assert_eq!(n, Some(native.clone()));
        *store.native.lock().map_err(|_| "mutex")? = None;
        assert!(
            read_stream_segment(&worker, &store, &s, p.id(), f.catalogs(), &access)
                .await
                .is_err()
        );
        *store.native.lock().map_err(|_| "mutex")? = Some(native.clone());
        owner
            .withdraw_stream_data(f.account()?, &s.data().id)
            .await?;
        assert!(
            read_stream_publication(&worker, &store, &s, p.id(), f.catalogs(), &access)
                .await
                .is_err()
        );
        owner
            .confirm_stream_writers_quiescent(f.account()?, &s.data().id)
            .await?;
        owner
            .retire_stream_segment(f.account()?, &s.data().id, p.id())
            .await?;
        let retired:(bool,bool)=sqlx::query_as("SELECT preparation IS NULL AND event IS NULL,descriptor<>'' FROM edgeagent_stream_publications").fetch_one(&h.pool).await?;
        assert_eq!(retired, (true, true));
        store.fail.store(true, Ordering::SeqCst);
        assert!(
            owner
                .cleanup_stream_segments(f.account()?, &s.data().id, 1, &store, &Quiet(true))
                .await
                .is_err()
        );
        let descriptor: String =
            sqlx::query_scalar("SELECT descriptor FROM edgeagent_stream_publications")
                .fetch_one(&h.pool)
                .await?;
        assert_eq!(
            StreamPublication::objects(&descriptor)?.1,
            Some(native.object())
        );
        assert_eq!(
            owner
                .cleanup_stream_segments(f.account()?, &s.data().id, 1, &store, &Quiet(true))
                .await?,
            1
        );
        let descriptor: String =
            sqlx::query_scalar("SELECT descriptor FROM edgeagent_stream_publications")
                .fetch_one(&h.pool)
                .await?;
        assert!(descriptor.is_empty());
        assert!(store.segment.lock().map_err(|_| "mutex")?.is_none());
        Ok(())
    };
    let result = AssertUnwindSafe(operation).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
