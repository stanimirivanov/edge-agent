use super::*;
use bytes::Bytes;
use edgeagent_contracts::evidence::{DailyEvidence, EvidenceId, EvidenceRevision};
use edgeagent_market_data_domain::UtcInstant;
use futures_util::{StreamExt, stream};
use object_store::{
    local::LocalFileSystem,
    memory::InMemory,
    throttle::{ThrottleConfig, ThrottledStore},
};
use std::{
    error::Error as StdError,
    sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
};

type TestResult = Result<(), Box<dyn StdError>>;
mod native;
struct CleanupOwner(bool, EvidenceObject);
impl CleanupAuthority for CleanupOwner {
    fn authorize_deletion(
        &self,
        request: &EvidenceId,
        object: &EvidenceObject,
    ) -> Result<(), edgeagent_market_data_application::BudgetError> {
        self.confirm_writes_quiescent(request)?;
        if object == &self.1 {
            Ok(())
        } else {
            Err(edgeagent_market_data_application::BudgetError::new(
                edgeagent_market_data_application::BudgetErrorKind::Denied,
            ))
        }
    }
    fn confirm_writes_quiescent(
        &self,
        _: &EvidenceId,
    ) -> Result<(), edgeagent_market_data_application::BudgetError> {
        if self.0 {
            Ok(())
        } else {
            Err(edgeagent_market_data_application::BudgetError::new(
                edgeagent_market_data_application::BudgetErrorKind::Denied,
            ))
        }
    }
    fn confirm_account_quiescent(
        &self,
        id: &EvidenceId,
        _: i64,
    ) -> Result<(), edgeagent_market_data_application::BudgetError> {
        self.confirm_writes_quiescent(id)
    }
}
#[tokio::test(start_paused = true)]
async fn quiescent_deletion_is_scoped_idempotent_and_recovers_after_timeout() -> TestResult {
    let memory = Arc::new(InMemory::new());
    let slow = Arc::new(ThrottledStore::new(
        memory.clone(),
        ThrottleConfig::default(),
    ));
    let store = adapter(slow.clone())?;
    let a = fixture()?;
    let o = descriptor(&a)?;
    let id = &a.data().scope.request;
    store.put(&a, &Access::new(o.clone())).await?;
    let mut other = o.clone();
    other.audience = EvidenceId::new(&"3".repeat(32))?;
    memory.put(&key(&other), a.bytes().to_vec().into()).await?;
    assert_kind(
        store.delete(id, &o, &CleanupOwner(false, o.clone())).await,
        Kind::Denied,
    )?;
    assert!(memory.head(&key(&o)).await.is_ok());
    slow.config_mut(|c| c.wait_delete_per_call = Duration::from_secs(2));
    assert_kind(
        store.delete(id, &o, &CleanupOwner(true, o.clone())).await,
        Kind::Deadline,
    )?;
    slow.config_mut(|c| c.wait_delete_per_call = Duration::ZERO);
    let restarted = adapter(memory.clone())?;
    let owner = CleanupOwner(true, o.clone());
    assert_kind(restarted.delete(id, &other, &owner).await, Kind::Denied)?;
    restarted.delete(id, &o, &owner).await?;
    restarted.delete(id, &o, &owner).await?;
    assert!(memory.head(&key(&o)).await.is_err());
    assert!(memory.head(&key(&other)).await.is_ok());
    Ok(())
}
fn assert_kind<T>(result: Result<T, Error>, kind: Kind) -> TestResult {
    assert_eq!(result.err().ok_or("expected storage failure")?.kind(), kind);
    Ok(())
}
struct Access {
    allowed: AtomicBool,
    now: AtomicI64,
    expected: EvidenceObject,
}
impl Access {
    fn new(object: EvidenceObject) -> Self {
        Self {
            allowed: AtomicBool::new(true),
            now: AtomicI64::new(172_800_000),
            expected: object,
        }
    }
}
impl EvidenceAccess for Access {
    #[allow(clippy::unwrap_used)] // Test-only clock is assigned valid fixed UTC instants below.
    fn now(&self) -> UtcInstant {
        UtcInstant::from_unix_millis(self.now.load(Ordering::SeqCst)).unwrap()
    }
    fn authorize(&self, object: &EvidenceObject) -> Result<(), Error> {
        if self.allowed.load(Ordering::SeqCst) && object == &self.expected {
            Ok(())
        } else {
            Err(Error::new(Kind::Denied))
        }
    }
}
fn fixture() -> Result<DailyEvidenceArtifact, Box<dyn StdError>> {
    let data: DailyEvidence = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../contracts/fixtures/evidence-v1/daily.json"
    )))?;
    Ok(DailyEvidenceArtifact::new(data)?)
}
fn adapter(backend: Arc<dyn ObjectStore>) -> Result<ImmutableEvidenceStore, Error> {
    ImmutableEvidenceStore::new(backend, Duration::from_secs(1))
}

#[tokio::test]
async fn concurrent_creates_and_corrections_preserve_exact_immutable_bytes() -> TestResult {
    let backend = Arc::new(InMemory::new());
    let store = adapter(backend.clone())?;
    let artifact = fixture()?;
    let expected = descriptor(&artifact)?;
    let access = Access::new(expected.clone());
    let (left, right) = tokio::join!(store.put(&artifact, &access), store.put(&artifact, &access));
    assert_eq!(left?, right?);
    assert_eq!(
        store.get(&expected, &access).await?.bytes(),
        artifact.bytes()
    );
    let mut correction = artifact.data().clone();
    correction.scope.request = EvidenceId::new(&"2".repeat(32))?;
    correction.replaces = Some(artifact.digest().clone());
    let correction = DailyEvidenceArtifact::new(correction)?;
    let corrected = descriptor(&correction)?;
    store
        .put(&correction, &Access::new(corrected.clone()))
        .await?;
    assert_ne!(key(&expected), key(&corrected));
    assert_eq!(store.get(&expected, &access).await?, artifact);
    assert_eq!(backend.list(None).try_collect::<Vec<_>>().await?.len(), 2);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn lost_write_completion_and_dropped_operations_recover_without_overwrite() -> TestResult {
    let memory = Arc::new(InMemory::new());
    let throttled = Arc::new(ThrottledStore::new(
        memory.clone(),
        ThrottleConfig {
            wait_get_per_call: Duration::from_secs(2),
            ..Default::default()
        },
    ));
    let store = adapter(throttled.clone())?;
    let artifact = fixture()?;
    let object = descriptor(&artifact)?;
    let access = Access::new(object.clone());
    throttled.config_mut(|config| config.wait_put_per_call = Duration::from_secs(2));
    let mut before_commit = Box::pin(store.put(&artifact, &access));
    assert!(futures_util::poll!(before_commit.as_mut()).is_pending());
    drop(before_commit);
    assert!(memory.head(&key(&object)).await.is_err());
    throttled.config_mut(|config| config.wait_put_per_call = Duration::ZERO);
    // PUT commits, then confirmation GET exceeds the whole-operation deadline.
    assert_kind(store.put(&artifact, &access).await, Kind::Deadline)?;
    assert!(memory.head(&key(&object)).await.is_ok());
    let mut operation = Box::pin(store.put(&artifact, &access));
    assert!(futures_util::poll!(operation.as_mut()).is_pending());
    drop(operation); // No Drop compensation or deletion is permitted.
    throttled.config_mut(|config| config.wait_get_per_call = Duration::ZERO);
    let restarted = adapter(memory.clone())?;
    assert_eq!(restarted.put(&artifact, &access).await?, object);
    assert_eq!(memory.list(None).try_collect::<Vec<_>>().await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn corrupt_collisions_descriptors_and_stream_bounds_fail_closed() -> TestResult {
    let memory = Arc::new(InMemory::new());
    let store = adapter(memory.clone())?;
    let artifact = fixture()?;
    let object = descriptor(&artifact)?;
    let access = Access::new(object.clone());
    assert_kind(store.get(&object, &access).await, Kind::NotFound)?;
    let mut corrupt = artifact.bytes().to_vec();
    corrupt[0] = b'!';
    memory.put(&key(&object), corrupt.clone().into()).await?;
    assert_kind(store.put(&artifact, &access).await, Kind::Integrity)?;
    assert_eq!(
        memory.get(&key(&object)).await?.bytes().await?.as_ref(),
        corrupt
    );
    memory
        .put(&key(&object), artifact.bytes().to_vec().into())
        .await?;
    let mut wrong = object.clone();
    wrong.delete_by.seconds -= 1;
    assert_kind(
        store.get(&wrong, &Access::new(wrong.clone())).await,
        Kind::Integrity,
    )?;
    wrong.bytes = 0;
    assert_kind(store.get(&wrong, &access).await, Kind::Invalid)?;
    wrong.bytes = MAX_EVIDENCE_BYTES as u32 + 1;
    assert_kind(store.get(&wrong, &access).await, Kind::Invalid)?;
    let oversized = stream::iter([Ok(Bytes::from(vec![0; object.bytes as usize + 1]))]).boxed();
    assert_kind(collect(oversized, &object, &access).await, Kind::Integrity)?;
    let truncated = stream::iter([Ok(Bytes::from_static(b"short"))]).boxed();
    assert_kind(collect(truncated, &object, &access).await, Kind::Integrity)?;
    let broken = stream::iter([Err(object_store::Error::Generic {
        store: "synthetic",
        source: Box::new(std::io::Error::other("private-backend-sentinel")),
    })])
    .boxed();
    let error = collect(broken, &object, &access)
        .await
        .err()
        .ok_or("expected storage failure")?;
    assert_eq!(error.kind(), Kind::Backend);
    assert!(error.source().is_some());
    assert!(!format!("{error:?} {error}").contains("private-backend-sentinel"));
    let empty_forever = stream::repeat_with(|| Ok(Bytes::new())).boxed();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(1),
            collect(empty_forever, &object, &access)
        )
        .await
        .is_err()
    );
    // Correct content hash cannot hide alternate canonical encoding.
    let mut alternate = artifact.bytes().to_vec();
    alternate.push(b' ');
    let mut other = object.clone();
    other.sha256 = DailyEvidenceArtifact::hash(&alternate);
    other.bytes += 1;
    let input = stream::iter([Ok(Bytes::from(alternate))]).boxed();
    assert_kind(
        collect(input, &other, &Access::new(other.clone())).await,
        Kind::Integrity,
    )?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn current_authority_and_exclusive_expiry_are_rechecked_after_io() -> TestResult {
    let memory = Arc::new(InMemory::new());
    let throttled = Arc::new(ThrottledStore::new(
        memory.clone(),
        ThrottleConfig::default(),
    ));
    let store = adapter(throttled.clone())?;
    let artifact = fixture()?;
    let object = descriptor(&artifact)?;
    let access = Access::new(object.clone());
    store.put(&artifact, &access).await?;
    access.allowed.store(false, Ordering::SeqCst);
    assert_kind(store.get(&object, &access).await, Kind::Denied)?;
    assert_kind(store.put(&artifact, &access).await, Kind::Denied)?;
    access.allowed.store(true, Ordering::SeqCst);
    throttled.config_mut(|c| c.wait_get_per_call = Duration::from_millis(100));
    let mut operation = Box::pin(store.get(&object, &access));
    assert!(futures_util::poll!(operation.as_mut()).is_pending());
    access.allowed.store(false, Ordering::SeqCst);
    assert_kind(operation.await, Kind::Denied)?;
    access.allowed.store(true, Ordering::SeqCst);
    let mut operation = Box::pin(store.get(&object, &access));
    assert!(futures_util::poll!(operation.as_mut()).is_pending());
    access
        .now
        .store(object.delete_by.seconds * 1000, Ordering::SeqCst);
    assert_kind(operation.await, Kind::Expired)?;
    assert_kind(store.get(&object, &access).await, Kind::Expired)?;
    assert!(memory.head(&key(&object)).await.is_ok()); // Access denial is not physical deletion.
    Ok(())
}

#[tokio::test]
async fn durable_local_store_reopens_and_licensed_namespaces_do_not_alias() -> TestResult {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "edgeagent-evidence-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root)?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let cleanup = Cleanup(root.clone());
    let artifact = fixture()?;
    let object = descriptor(&artifact)?;
    let access = Access::new(object.clone());
    // This proof exercises real filesystem persistence, not a one-second latency SLA.
    // Keep the production maximum; memory/virtual-time deadline proofs retain their short budget.
    let local_timeout = Duration::from_secs(15);
    {
        let store = ImmutableEvidenceStore::new(
            Arc::new(LocalFileSystem::new_with_prefix(&root)?),
            local_timeout,
        )?;
        store.put(&artifact, &access).await?;
    }
    let store = ImmutableEvidenceStore::new(
        Arc::new(LocalFileSystem::new_with_prefix(&root)?),
        local_timeout,
    )?;
    assert_eq!(store.get(&object, &access).await?, artifact);
    for variant in 0..4 {
        let mut other = object.clone();
        match variant {
            0 => other.storage = EvidenceId::new(&"2".repeat(32))?,
            1 => other.audience = EvidenceId::new(&"2".repeat(32))?,
            2 => other.rights.id = EvidenceId::new(&"2".repeat(32))?,
            _ => other.rights.revision = EvidenceRevision::new("2")?,
        }
        assert_ne!(key(&object), key(&other));
        assert_kind(
            store.get(&other, &Access::new(other.clone())).await,
            Kind::NotFound,
        )?;
    }
    drop(store);
    std::fs::remove_dir_all(&root)?;
    std::mem::forget(cleanup);
    Ok(())
}

#[tokio::test]
async fn storage_errors_and_adapter_debug_redact_paths_hashes_and_payloads() -> TestResult {
    let backend = Arc::new(InMemory::new());
    assert_kind(
        ImmutableEvidenceStore::new(backend.clone(), Duration::ZERO),
        Kind::Invalid,
    )?;
    assert_kind(
        ImmutableEvidenceStore::new(backend.clone(), Duration::from_secs(16)),
        Kind::Invalid,
    )?;
    let store = adapter(backend)?;
    let artifact = fixture()?;
    let object = descriptor(&artifact)?;
    let error = store
        .get(&object, &Access::new(object.clone()))
        .await
        .err()
        .ok_or("expected storage failure")?;
    assert!(error.source().is_some());
    let text = format!("{error} {error:?} {store:?}");
    assert!(!text.contains(object.sha256.as_str()));
    assert!(!text.contains("evidence/daily-v1/"));
    assert!(!text.contains("123456789"));
    let error = Error::with_source(Kind::Backend, std::io::Error::other("secret-payload"));
    assert!(!format!("{error} {error:?}").contains("secret-payload"));
    assert!(error.source().is_some());
    Ok(())
}
