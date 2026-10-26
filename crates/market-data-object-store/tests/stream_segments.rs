//! Invented evidence and memory storage only; no provider or cloud access.
#[allow(dead_code)]
#[path = "../../market-data-application/tests/support/segment.rs"]
mod fixture;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_object_store::ImmutableEvidenceStore;
use fixture::*;
use object_store::{
    ObjectStore, ObjectStoreExt,
    memory::InMemory,
    path::Path,
    throttle::{ThrottleConfig, ThrottledStore},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
    },
    time::Duration,
};
type K = EvidenceStorageErrorKind;
struct Access {
    object: StreamSegmentObject,
    allowed: AtomicBool,
    now: AtomicI64,
    checks: AtomicUsize,
    deny_after: usize,
}
impl Access {
    fn new(object: StreamSegmentObject) -> Self {
        Self {
            object,
            allowed: AtomicBool::new(true),
            now: AtomicI64::new(AT + 100),
            checks: AtomicUsize::new(0),
            deny_after: usize::MAX,
        }
    }
}
impl StreamSegmentAccess for Access {
    fn now(&self) -> edgeagent_market_data_domain::UtcInstant {
        // Assigned exclusively from known-valid synthetic epoch values below.
        #[allow(clippy::unwrap_used)]
        edgeagent_market_data_domain::UtcInstant::from_unix_millis(self.now.load(Ordering::SeqCst))
            .unwrap()
    }
    fn authorize(&self, o: &StreamSegmentObject) -> std::result::Result<(), EvidenceStorageError> {
        if self.allowed.load(Ordering::SeqCst)
            && self.object == *o
            && self.checks.fetch_add(1, Ordering::SeqCst) < self.deny_after
        {
            Ok(())
        } else {
            Err(EvidenceStorageError::new(K::Denied))
        }
    }
}
fn adapter(backend: Arc<dyn ObjectStore>) -> Result<ImmutableEvidenceStore> {
    Ok(ImmutableEvidenceStore::new(
        backend,
        Duration::from_secs(1),
    )?)
}
fn key(o: &StreamSegmentObject) -> Path {
    let s = &o.scope;
    Path::from(format!(
        "evidence/stream-segment-v1/{}/{}/{}/{}/{}/{}.json",
        s.storage.as_str(),
        s.audience.as_str(),
        s.rights.id.as_str(),
        s.rights.revision.as_str(),
        s.session.as_str(),
        o.sha256.as_str()
    ))
}
fn kind<T>(r: std::result::Result<T, EvidenceStorageError>, k: K) -> Result {
    assert_eq!(r.err().ok_or("failure expected")?.kind(), k);
    Ok(())
}
#[tokio::test]
async fn stream_segment_store_is_create_only_scoped_and_reopens() -> Result {
    let backend = Arc::new(InMemory::new());
    let store = adapter(backend.clone())?;
    let a = segment()?;
    let o = a.object();
    let access = Access::new(o.clone());
    let (l, r) = tokio::join!(
        store.put_segment(&a, &access),
        store.put_segment(&a, &access)
    );
    assert_eq!(l?, o);
    assert_eq!(r?, o);
    assert_eq!(adapter(backend.clone())?.get_segment(&o, &access).await?, a);
    let mut data = a.data().clone();
    data.scope.audience = EvidenceId::new(&format!("{:032x}", 99))?;
    let other = StreamSegment::new(data)?;
    assert_ne!(key(&other.object()), key(&o));
    store
        .put_segment(&other, &Access::new(other.object()))
        .await?;
    kind(store.get_segment(&other.object(), &access).await, K::Denied)?;
    // A daily descriptor cannot route into the independently versioned streaming namespace.
    let daily = EvidenceObject {
        sha256: o.sha256.clone(),
        bytes: o.bytes,
        storage: o.scope.storage.clone(),
        audience: o.scope.audience.clone(),
        rights: o.scope.rights.clone(),
        delete_by: o.delete_by,
        replaces: None,
    };
    struct Daily(edgeagent_market_data_domain::UtcInstant);
    impl EvidenceAccess for Daily {
        fn now(&self) -> edgeagent_market_data_domain::UtcInstant {
            self.0
        }
        fn authorize(&self, _: &EvidenceObject) -> std::result::Result<(), EvidenceStorageError> {
            Ok(())
        }
    }
    kind(store.get(&daily, &Daily(at(AT + 100)?)).await, K::NotFound)?;
    assert!(backend.head(&key(&o)).await.is_ok());
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn stream_segment_store_recovers_uncertain_put_and_denies_revocation_expiry() -> Result {
    let backend = Arc::new(InMemory::new());
    let slow = Arc::new(ThrottledStore::new(
        backend.clone(),
        ThrottleConfig::default(),
    ));
    slow.config_mut(|c| c.wait_get_per_call = Duration::from_secs(2));
    let a = segment()?;
    let o = a.object();
    let access = Access::new(o.clone());
    // PUT commits; its mandatory read-back times out. Retry exact bytes after reconstruction.
    kind(adapter(slow)?.put_segment(&a, &access).await, K::Deadline)?;
    assert!(backend.head(&key(&o)).await.is_ok());
    let store = adapter(backend.clone())?;
    assert_eq!(store.put_segment(&a, &access).await?, o);
    access.allowed.store(false, Ordering::SeqCst);
    kind(store.get_segment(&o, &access).await, K::Denied)?;
    access.allowed.store(true, Ordering::SeqCst);
    access.now.store(AT + 1000, Ordering::SeqCst);
    kind(store.get_segment(&o, &access).await, K::Expired)?;
    kind(store.put_segment(&a, &access).await, K::Expired)?;
    let mut changed = Access::new(o.clone());
    changed.deny_after = 1;
    kind(store.get_segment(&o, &changed).await, K::Denied)?; // Denied after backend I/O.
    assert!(backend.head(&key(&o)).await.is_ok()); // Never compensate by deleting.
    Ok(())
}
#[tokio::test]
async fn stream_segment_store_rejects_descriptor_and_content_attacks() -> Result {
    let backend = Arc::new(InMemory::new());
    let store = adapter(backend.clone())?;
    let a = segment()?;
    let o = a.object();
    let access = Access::new(o.clone());
    store.put_segment(&a, &access).await?;
    let mut wrong = o.clone();
    wrong.bytes += 1;
    kind(
        store.get_segment(&wrong, &Access::new(wrong.clone())).await,
        K::Integrity,
    )?;
    let mut wrong = o.clone();
    wrong.delete_by.seconds += 1;
    kind(
        store.get_segment(&wrong, &Access::new(wrong.clone())).await,
        K::Integrity,
    )?;
    let mut wrong = o.clone();
    wrong.bytes = MAX_STREAM_SEGMENT_BYTES as u32 + 1;
    kind(
        store.get_segment(&wrong, &Access::new(wrong.clone())).await,
        K::Invalid,
    )?;
    let mut corrupt = a.bytes().to_vec();
    if let Some(b) = corrupt.first_mut() {
        *b = b'!';
    }
    backend.put(&key(&o), corrupt.into()).await?;
    kind(store.get_segment(&o, &access).await, K::Integrity)?;
    kind(store.put_segment(&a, &access).await, K::Integrity)?; // Collision is not repaired.
    assert_ne!(
        backend.get(&key(&o)).await?.bytes().await?.as_ref(),
        a.bytes()
    );
    Ok(())
}
