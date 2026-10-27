//! Real isolated SQL transactions and worker LOGINs; no transport or licensed inputs.
#![allow(
    clippy::disallowed_methods,
    reason = "Isolated schema administration and synthetic fault injection only."
)]
#[allow(dead_code)]
#[path = "../../market-data-application/tests/support/streaming.rs"]
mod fixture;
#[allow(dead_code)]
mod support;
use edgeagent_contracts::{Component, MessageMetadata, evidence::*};
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_postgres::PostgresStreamingLedger as Ledger;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use support::*;
struct SegmentStore {
    stored: std::sync::Mutex<Option<StreamSegment>>,
    fail: std::sync::atomic::AtomicBool,
    puts: std::sync::atomic::AtomicUsize,
}
impl StreamSegmentStorage for SegmentStore {
    async fn put_segment(
        &self,
        segment: &StreamSegment,
        access: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamSegmentObject, EvidenceStorageError> {
        access.authorize(&segment.object())?;
        self.puts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self
            .stored
            .lock()
            .map_err(|_| EvidenceStorageError::new(EvidenceStorageErrorKind::Backend))? =
            Some(segment.clone());
        if self.fail.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(EvidenceStorageError::new(
                EvidenceStorageErrorKind::Deadline,
            ));
        }
        Ok(segment.object())
    }
    async fn get_segment(
        &self,
        object: &StreamSegmentObject,
        access: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamSegment, EvidenceStorageError> {
        access.authorize(object)?;
        self.stored
            .lock()
            .map_err(|_| EvidenceStorageError::new(EvidenceStorageErrorKind::Backend))?
            .clone()
            .ok_or_else(|| EvidenceStorageError::new(EvidenceStorageErrorKind::NotFound))
    }
}
struct Access {
    object: StreamSegmentObject,
    at: UtcInstant,
}
struct WithdrawDuringRead<'a> {
    owner: &'a Ledger,
    account: ProviderAccountId,
    session: EvidenceId,
    segment: StreamSegment,
}
impl StreamSegmentStorage for WithdrawDuringRead<'_> {
    async fn put_segment(
        &self,
        _: &StreamSegment,
        _: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamSegmentObject, EvidenceStorageError> {
        Err(EvidenceStorageError::new(EvidenceStorageErrorKind::Denied))
    }
    async fn get_segment(
        &self,
        object: &StreamSegmentObject,
        access: &impl StreamSegmentAccess,
    ) -> std::result::Result<StreamSegment, EvidenceStorageError> {
        access.authorize(object)?;
        self.owner
            .withdraw_stream_data(self.account, &self.session)
            .await
            .map_err(|e| EvidenceStorageError::with_source(EvidenceStorageErrorKind::Backend, e))?;
        Ok(self.segment.clone())
    }
}
impl StreamSegmentAccess for Access {
    fn now(&self) -> UtcInstant {
        self.at
    }
    fn authorize(&self, o: &StreamSegmentObject) -> std::result::Result<(), EvidenceStorageError> {
        if *o == self.object {
            Ok(())
        } else {
            Err(EvidenceStorageError::new(EvidenceStorageErrorKind::Denied))
        }
    }
}

fn proposal(
    f: &fixture::Fixture,
    s: &StreamingSession,
    c: &StreamingConnection,
    id: u128,
    frames: [u64; 2],
    previous: Option<&EvidenceDigest>,
) -> Result<StreamPublication> {
    let p = EvidenceId::new(&format!("{id:032x}"))?;
    // Control-only captures prove accounting/chain behavior, never market readiness.
    let segment = freeze_stream_segment(
        s,
        c,
        f.catalogs(),
        fixture::at(f.ms + 1)?,
        &Cancellation::default(),
        StreamSegmentInput {
            instrument: InstrumentId::new(1)?,
            epoch: FeedEpoch::new(1)?,
            frames,
            previous,
            limits: StreamSegmentLimits {
                bytes: 65536,
                records: 16,
            },
            delete_by: fixture::at(f.ms + 600_000)?,
            records: &[],
        },
    )?;
    Ok(StreamPublication::new(
        p.clone(),
        segment,
        MessageMetadata {
            id: format!("stream-publication-{}", p.as_str()),
            source: Component::MarketData.source_uri().into(),
            message_type: STREAM_PUBLISHED.message_type().into(),
            subject: format!("stream/{}", s.data().id.as_str()),
            time: "2026-10-26T00:00:00Z".into(),
            data_schema: STREAM_PUBLISHED.data_schema().into(),
            correlation_id: p.as_str().into(),
            causation_id: s.data().id.as_str().into(),
            idempotency_key: p.as_str().into(),
            partition_key: format!("stream/{}", s.data().id.as_str()),
            trace_parent: "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01".into(),
            trace_state: None,
        },
    )?)
}
async fn charge(
    w: &Ledger,
    f: &fixture::Fixture,
    s: &StreamingSession,
    c: &StreamingConnection,
    n: u64,
) -> Result {
    w.charge(
        s,
        c,
        f.catalogs(),
        &Cancellation::default(),
        StreamingArrival {
            ordinal: n,
            bytes: 0,
        },
    )
    .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_restore_requires_independent_approval_and_preserves_sticky_incidents() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result = AssertUnwindSafe(async {
        let f = fixture::Fixture::new(h.now - 100, 1)?;
        let s = f.session(1)?;
        let owner = Ledger::new(h.pool.clone());
        let w = Ledger::new(pool.clone());
        owner
            .configure_account(f.account()?, f.policy(1, 4)?, 0)
            .await?;
        owner.admit(&s, f.catalogs()).await?;
        assert_eq!(
            w.recovery_state(&s, f.catalogs()).await?,
            StreamRecoveryState {
                revision: 0,
                blocked: true,
                incident: None
            }
        );
        let restore = w.begin_restore(&s, f.catalogs()).await?;
        assert!(
            w.check_stream_restored(&s, &restore, f.catalogs())
                .await
                .is_err()
        );
        assert!(
            w.approve_stream_restore(&s, &restore, f.catalogs())
                .await
                .is_err()
        );
        assert!(
            sqlx::query("UPDATE edgeagent_stream_recovery SET blocked=FALSE")
                .execute(&pool)
                .await
                .is_err()
        );
        owner
            .approve_stream_restore(&s, &restore, f.catalogs())
            .await?;
        w.check_stream_restored(&s, &restore, f.catalogs()).await?;
        let before = w.status(&s.data().id).await?;
        let restarted = Ledger::new(pool.clone());
        let next = restarted.begin_restore(&s, f.catalogs()).await?;
        assert!(next.identity().1 > restore.identity().1);
        assert!(
            w.check_stream_restored(&s, &restore, f.catalogs())
                .await
                .is_err()
        );
        let state = restarted
            .record_stream_incident(&s, &next, StreamStop::SourceFailure, f.catalogs())
            .await?;
        assert!(state.blocked);
        assert_eq!(state.incident, Some(StreamStop::SourceFailure));
        assert_eq!(
            Ledger::new(pool.clone())
                .recovery_state(&s, f.catalogs())
                .await?,
            state
        );
        assert!(
            owner
                .approve_stream_restore(&s, &next, f.catalogs())
                .await
                .is_err()
        );
        assert!(restarted.begin_restore(&s, f.catalogs()).await.is_err());
        assert_eq!(w.status(&s.data().id).await?, before);
        assert_eq!(format!("{next:?}"), "StreamRestore(..)");
        Ok(())
    })
    .catch_unwind()
    .await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_restore_recovers_exact_pending_intents_and_approval_rolls_back() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result=AssertUnwindSafe(async {
        let f=fixture::Fixture::new(h.now-100,1)?; let s=f.session(1)?;
        let owner=Ledger::new(h.pool.clone()); let w=Ledger::new(pool.clone()); let cancel=Cancellation::default();
        owner.configure_account(f.account()?,f.policy(1,4)?,0).await?;owner.admit(&s,f.catalogs()).await?;
        let c=w.reserve(&s,f.catalogs(),&cancel).await?;charge(&w,&f,&s,&c,1).await?;
        let p=proposal(&f,&s,&c,70,[1,1],None)?;
        w.stage(&s,&p,f.catalogs(),&cancel).await?;
        let before=w.status(&s.data().id).await?;
        let restarted=Ledger::new(pool.clone()); let restore=restarted.begin_restore(&s,f.catalogs()).await?;
        assert!(owner.approve_stream_restore(&s,&restore,f.catalogs()).await.is_err());
        let store=SegmentStore{stored:std::sync::Mutex::new(None),fail:std::sync::atomic::AtomicBool::new(true),puts:std::sync::atomic::AtomicUsize::new(0)};
        let access=Access{object:p.segment().object(),at:fixture::at(h.now)?};
        assert!(recover_pending_stream_segment(&restarted,&restarted,&store,&s,&restore,InstrumentId::new(1)?,f.catalogs(),&cancel,&access).await.is_err());
        assert!(restarted.recovery_state(&s,f.catalogs()).await?.blocked);
        assert_eq!(restarted.pending_segment(&s,InstrumentId::new(1)?,f.catalogs()).await?.ok_or("pending")?.bytes()?,p.bytes()?);
        let receipt=recover_pending_stream_segment(&restarted,&restarted,&store,&s,&restore,InstrumentId::new(1)?,f.catalogs(),&cancel,&access).await?.ok_or("receipt")?;
        // Model lost COMMIT confirmation: a fresh client discovers no pending work,
        // and recovers the retained exact receipt without another object PUT.
        let fresh=Ledger::new(pool.clone()); let puts=store.puts.load(std::sync::atomic::Ordering::SeqCst);
        assert!(recover_pending_stream_segment(&fresh,&fresh,&store,&s,&restore,InstrumentId::new(1)?,f.catalogs(),&cancel,&access).await?.is_none());
        assert_eq!(fresh.recover_publication(&s,&p,f.catalogs()).await?,Some(receipt));
        assert_eq!(store.puts.load(std::sync::atomic::Ordering::SeqCst),puts);
        sqlx::raw_sql("CREATE FUNCTION fail_restore() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic rollback'; END $$; CREATE TRIGGER fail_restore AFTER UPDATE ON edgeagent_stream_recovery FOR EACH ROW WHEN(NOT NEW.blocked) EXECUTE FUNCTION fail_restore();").execute(&h.pool).await?;
        assert!(owner.approve_stream_restore(&s,&restore,f.catalogs()).await.is_err());
        assert!(fresh.recovery_state(&s,f.catalogs()).await?.blocked);
        sqlx::raw_sql("DROP TRIGGER fail_restore ON edgeagent_stream_recovery; DROP FUNCTION fail_restore();").execute(&h.pool).await?;
        owner.approve_stream_restore(&s,&restore,f.catalogs()).await?;
        fresh.check_stream_restored(&s,&restore,f.catalogs()).await?;
        assert_eq!(fresh.status(&s.data().id).await?,before);
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_outbox").fetch_one(&h.pool).await?;assert_eq!(count,1);
        Ok(())
    }).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_preparation_and_atomic_publication_survive_result_loss_and_rollback() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result=AssertUnwindSafe(async {
        let f=fixture::Fixture::new(h.now-100,1)?;let s=f.session(1)?;
        let owner=Ledger::new(h.pool.clone());let w=Ledger::new(pool.clone());let cancel=Cancellation::default();
        owner.configure_account(f.account()?,f.policy(1,4)?,0).await?;owner.admit(&s,f.catalogs()).await?;
        let c=w.reserve(&s,f.catalogs(),&cancel).await?;charge(&w,&f,&s,&c,1).await?;
        let p=proposal(&f,&s,&c,10,[1,1],None)?;
        let store=SegmentStore{stored:std::sync::Mutex::new(None),fail:std::sync::atomic::AtomicBool::new(true),puts:std::sync::atomic::AtomicUsize::new(0)};
        let access=Access{object:p.segment().object(),at:fixture::at(h.now)?};
        assert!(publish_stream_segment(&w,&store,&s,&p,f.catalogs(),&cancel,&access).await.is_err());
        let restarted=Ledger::new(pool.clone());
        let recovered=restarted.pending_segment(&s,InstrumentId::new(1)?,f.catalogs()).await?.ok_or("missing preparation")?;
        assert_eq!(recovered.id(),p.id());
        assert_eq!(restarted.recover_segment(&s,recovered.id(),f.catalogs()).await?.ok_or("missing exact recovery")?.bytes()?,p.bytes()?);
        assert_eq!(recovered.bytes()?,p.bytes()?);
        // Fault after outbox enqueue: head, event, receipt must all roll back.
        sqlx::raw_sql("CREATE FUNCTION fail_stream_publish() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic fault'; END $$; CREATE TRIGGER fail_stream_publish AFTER UPDATE ON edgeagent_stream_publications FOR EACH ROW WHEN(NEW.published) EXECUTE FUNCTION fail_stream_publish();").execute(&h.pool).await?;
        kind(w.commit_segment(&s,&p,f.catalogs(),&cancel).await,BudgetErrorKind::Storage)?;
        let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM edgeagent_stream_heads),(SELECT count(*) FROM edgeagent_message_outbox)").fetch_one(&h.pool).await?;
        assert_eq!(counts,(0,0));assert!(w.recover_publication(&s,&p,f.catalogs()).await?.is_none());
        sqlx::raw_sql("DROP TRIGGER fail_stream_publish ON edgeagent_stream_publications; DROP FUNCTION fail_stream_publish();").execute(&h.pool).await?;
        let receipt=publish_stream_segment(&w,&store,&s,&recovered,f.catalogs(),&cancel,&access).await?;
        assert!(receipt.history_start);assert!(!receipt.skipped);
        assert_eq!(w.commit_segment(&s,&p,f.catalogs(),&cancel).await?,receipt);
        assert_eq!(w.recover_publication(&s,&p,f.catalogs()).await?,Some(receipt));
        let puts=store.puts.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(publish_stream_segment(&w,&store,&s,&recovered,f.catalogs(),&cancel,&access).await?,receipt);
        assert_eq!(store.puts.load(std::sync::atomic::Ordering::SeqCst),puts);
        assert_eq!(read_stream_segment(&w,&store,&s,p.id(),f.catalogs(),&access).await?,*p.segment());
        let payload:serde_json::Value=p.envelope().payload()?;
        assert_eq!(payload.as_object().ok_or("object")?.len(),2);
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_outbox").fetch_one(&h.pool).await?;assert_eq!(count,1);
        charge(&w,&f,&s,&c,2).await?;
        let successor=proposal(&f,&s,&c,11,[2,2],Some(p.segment().digest()))?;
        w.stage(&s,&successor,f.catalogs(),&cancel).await?;
        let next=w.commit_segment(&s,&successor,f.catalogs(),&cancel).await?;
        assert!(!next.history_start && !next.reconnect && !next.skipped);
        Ok(())
    }).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_publication_fences_generations_charged_ranges_and_chain_forks() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result = AssertUnwindSafe(async {
        let f = fixture::Fixture::new(h.now - 100, 1)?;
        let s = f.session(1)?;
        let owner = Ledger::new(h.pool.clone());
        let w = Ledger::new(pool.clone());
        let cancel = Cancellation::default();
        owner
            .configure_account(f.account()?, f.policy(1, 4)?, 0)
            .await?;
        owner.admit(&s, f.catalogs()).await?;
        let c = w.reserve(&s, f.catalogs(), &cancel).await?;
        let p = proposal(&f, &s, &c, 10, [1, 1], None)?;
        kind(
            w.stage(&s, &p, f.catalogs(), &cancel).await,
            BudgetErrorKind::Denied,
        )?;
        charge(&w, &f, &s, &c, 1).await?;
        w.stage(&s, &p, f.catalogs(), &cancel).await?;
        w.commit_segment(&s, &p, f.catalogs(), &cancel).await?;
        // Active retirement removes content history, not permission to spend frames again.
        sqlx::query("UPDATE edgeagent_stream_publications SET delete_ms=0")
            .execute(&h.pool)
            .await?;
        owner
            .retire_stream_segment(f.account()?, &s.data().id, p.id())
            .await?;
        let reused = proposal(&f, &s, &c, 13, [1, 1], None)?;
        kind(
            w.stage(&s, &reused, f.catalogs(), &cancel).await,
            BudgetErrorKind::Conflict,
        )?;
        owner
            .confirm_quiescent(f.account()?, &s.data().id, c.generation())
            .await?;
        sqlx::query("UPDATE edgeagent_stream_accounts SET last_start_ms=NULL")
            .execute(&h.pool)
            .await?;
        let next = w.reserve(&s, f.catalogs(), &cancel).await?;
        charge(&w, &f, &s, &next, 2).await?;
        charge(&w, &f, &s, &next, 3).await?;
        let old = proposal(&f, &s, &next, 11, [1, 1], Some(p.segment().digest()))?;
        kind(
            w.stage(&s, &old, f.catalogs(), &cancel).await,
            BudgetErrorKind::Denied,
        )?;
        let stale = proposal(&f, &s, &c, 11, [2, 2], Some(p.segment().digest()))?;
        kind(
            w.stage(&s, &stale, f.catalogs(), &cancel).await,
            BudgetErrorKind::Denied,
        )?;
        let nextp = proposal(&f, &s, &next, 11, [3, 3], None)?;
        w.stage(&s, &nextp, f.catalogs(), &cancel).await?;
        let b = w.commit_segment(&s, &nextp, f.catalogs(), &cancel).await?;
        assert!(b.reconnect && b.skipped && b.history_start && b.history_retired);
        let fork = proposal(&f, &s, &next, 12, [3, 3], None)?;
        kind(
            w.stage(&s, &fork, f.catalogs(), &cancel).await,
            BudgetErrorKind::Conflict,
        )?;
        kind(
            w.commit_segment(&s, &fork, f.catalogs(), &cancel).await,
            BudgetErrorKind::Conflict,
        )?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM edgeagent_stream_heads")
            .fetch_one(&h.pool)
            .await?;
        assert_eq!(count, 1);
        // Migration-era unknown generation boundary must fail closed without refund.
        sqlx::query("UPDATE edgeagent_stream_sessions SET generation_start=NULL")
            .execute(&h.pool)
            .await?;
        kind(
            w.stage(&s, &fork, f.catalogs(), &cancel).await,
            BudgetErrorKind::Denied,
        )?;
        assert!(owner.status(&s.data().id).await?.unresolved);
        Ok(())
    })
    .catch_unwind()
    .await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
struct Delete {
    calls: std::sync::atomic::AtomicUsize,
}
impl StreamDeletion for Delete {
    async fn delete_segment(
        &self,
        c: &StreamCleanup,
        a: &impl StreamDeletionAuthority,
    ) -> std::result::Result<(), EvidenceStorageError> {
        a.confirm_storage_quiescent(c)?;
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}
struct Quiet(bool);
impl StreamDeletionAuthority for Quiet {
    fn confirm_storage_quiescent(
        &self,
        _: &StreamCleanup,
    ) -> std::result::Result<(), EvidenceStorageError> {
        if self.0 {
            Ok(())
        } else {
            Err(EvidenceStorageError::new(EvidenceStorageErrorKind::Denied))
        }
    }
}
#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_stop_preserves_reads_withdrawal_denies_and_cleanup_requires_writer_quiescence()
-> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result=AssertUnwindSafe(async {
        let f=fixture::Fixture::new(h.now-100,1)?;let s=f.session(1)?;
        let owner=Ledger::new(h.pool.clone());let w=Ledger::new(pool.clone());let cancel=Cancellation::default();
        owner.configure_account(f.account()?,f.policy(1,4)?,0).await?;owner.admit(&s,f.catalogs()).await?;
        let c=w.reserve(&s,f.catalogs(),&cancel).await?;charge(&w,&f,&s,&c,1).await?;
        let p=proposal(&f,&s,&c,10,[1,1],None)?;
        w.stage(&s,&p,f.catalogs(),&cancel).await?;w.commit_segment(&s,&p,f.catalogs(),&cancel).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("GRANT UPDATE(denied) ON edgeagent_stream_retention TO {role}"))).execute(&h.pool).await?;
        kind(w.published_segment(&s,p.id(),f.catalogs()).await,BudgetErrorKind::Denied)?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("REVOKE UPDATE(denied) ON edgeagent_stream_retention FROM {role}"))).execute(&h.pool).await?;
        let revoked=f.revoked()?;
        kind(w.published_segment(&s,p.id(),AuthorityCatalogs{rights:&revoked,providers:&f.providers}).await,BudgetErrorKind::Denied)?;
        // A worker-local lookalike cannot override early-bound owner denial.
        sqlx::raw_sql("CREATE TEMP TABLE edgeagent_stream_retention(session_id text,denied bool,writers_quiet bool,independent bool)").execute(&pool).await?;
        sqlx::query("INSERT INTO pg_temp.edgeagent_stream_retention VALUES($1,FALSE,FALSE,TRUE)").bind(s.data().id.as_str()).execute(&pool).await?;
        owner.cancel(f.account()?,&s.data().id).await?;
        assert_eq!(w.published_segment(&s,p.id(),f.catalogs()).await?,p.segment().object());
        assert!(w.recover_publication(&s,&p,f.catalogs()).await?.is_some());
        kind(owner.retire_stream_segment(f.account()?,&s.data().id,p.id()).await,BudgetErrorKind::Denied)?;
        kind(w.withdraw_stream_data(f.account()?,&s.data().id).await,BudgetErrorKind::Denied)?;
        let revoking=WithdrawDuringRead{owner:&owner,account:f.account()?,session:s.data().id.clone(),segment:p.segment().clone()};
        let access=Access{object:p.segment().object(),at:fixture::at(h.now)?};
        kind(read_stream_segment(&w,&revoking,&s,p.id(),f.catalogs(),&access).await,BudgetErrorKind::Denied)?;
        kind(w.published_segment(&s,p.id(),f.catalogs()).await,BudgetErrorKind::Denied)?;
        owner.retire_stream_segment(f.account()?,&s.data().id,p.id()).await?;
        let remaining:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM edgeagent_stream_heads),(SELECT count(*) FROM edgeagent_message_outbox)").fetch_one(&h.pool).await?;assert_eq!(remaining,(0,0));
        let store=Delete{calls:std::sync::atomic::AtomicUsize::new(0)};
        kind(owner.cleanup_stream_segments(f.account()?,&s.data().id,16,&store,&Quiet(true)).await,BudgetErrorKind::Denied)?;
        owner.confirm_stream_writers_quiescent(f.account()?,&s.data().id).await?;
        assert!(owner.cleanup_stream_segments(f.account()?,&s.data().id,16,&store,&Quiet(false)).await.is_err());
        assert_eq!(owner.cleanup_stream_segments(f.account()?,&s.data().id,16,&store,&Quiet(true)).await?,1);
        assert_eq!(owner.cleanup_stream_segments(f.account()?,&s.data().id,16,&store,&Quiet(true)).await?,0);
        let bytes:Vec<u8>=sqlx::query_scalar("SELECT admission FROM edgeagent_stream_admissions").fetch_one(&h.pool).await?;assert_eq!(bytes,vec![0]);
        assert!(owner.status(&s.data().id).await?.unresolved);
        assert_eq!(owner.status(&s.data().id).await?.usage.frames,1);
        Ok(())
    }).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
