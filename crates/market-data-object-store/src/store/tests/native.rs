use super::*;

fn native() -> Result<NativeDailyArtifact, Box<dyn StdError>> {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../contracts/fixtures/evidence-date-only-v1/native-accepted.json"
    ));
    Ok(NativeDailyArtifact::decode(
        bytes,
        &DailyEvidenceArtifact::hash(bytes),
    )?)
}

#[tokio::test(start_paused = true)]
async fn native_uncertain_create_recovers_exact_bytes_and_rejects_other_formats() -> TestResult {
    let memory = Arc::new(InMemory::new());
    let slow = Arc::new(ThrottledStore::new(
        memory.clone(),
        ThrottleConfig {
            wait_get_per_call: Duration::from_secs(2),
            ..Default::default()
        },
    ));
    let store = adapter(slow.clone())?;
    let a = native()?;
    let object = a.object()?;
    let access = Access::new(object.clone());
    assert_kind(store.put_native(&a, &access).await, Kind::Deadline)?;
    assert!(memory.head(&key(&object)).await.is_ok());
    slow.config_mut(|c| c.wait_get_per_call = Duration::ZERO);
    let restarted = adapter(memory.clone())?;
    assert_eq!(restarted.put_native(&a, &access).await?, object);
    assert_eq!(restarted.get_native(&object, &access).await?, a);
    assert_kind(restarted.get(&object, &access).await, Kind::Integrity)?;
    let precise = fixture()?;
    let p = descriptor(&precise)?;
    restarted.put(&precise, &Access::new(p.clone())).await?;
    assert_kind(
        restarted.get_native(&p, &Access::new(p.clone())).await,
        Kind::Integrity,
    )?;
    let capture =
        edgeagent_contracts::evidence::DateOnlyDailyArtifact::new(a.data().capture.clone())?;
    let mut raw = object.clone();
    raw.sha256 = capture.digest().clone();
    raw.bytes = u32::try_from(capture.bytes().len())?;
    memory
        .put(&key(&raw), capture.bytes().to_vec().into())
        .await?;
    assert_kind(
        restarted.get_native(&raw, &Access::new(raw.clone())).await,
        Kind::Integrity,
    )?;
    // Matching length/hash cannot hide damaged canonical bytes under an accepted descriptor.
    memory
        .put(&key(&object), vec![0; object.bytes as usize].into())
        .await?;
    assert_kind(restarted.put_native(&a, &access).await, Kind::Integrity)?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn native_reads_recheck_authority_expiry_scope_and_quiescent_cleanup() -> TestResult {
    let memory = Arc::new(InMemory::new());
    let slow = Arc::new(ThrottledStore::new(
        memory.clone(),
        ThrottleConfig::default(),
    ));
    let store = adapter(slow.clone())?;
    let a = native()?;
    let object = a.object()?;
    let access = Access::new(object.clone());
    store.put_native(&a, &access).await?;
    let mut foreign = object.clone();
    foreign.audience = EvidenceId::new(&"3".repeat(32))?;
    assert_kind(store.get_native(&foreign, &access).await, Kind::Denied)?;
    slow.config_mut(|c| c.wait_get_per_call = Duration::from_millis(100));
    let mut read = Box::pin(store.get_native(&object, &access));
    assert!(futures_util::poll!(read.as_mut()).is_pending());
    access.allowed.store(false, Ordering::SeqCst);
    assert_kind(read.await, Kind::Denied)?;
    access.allowed.store(true, Ordering::SeqCst);
    let mut read = Box::pin(store.get_native(&object, &access));
    assert!(futures_util::poll!(read.as_mut()).is_pending());
    access.now.store(
        object.delete_by.seconds * 1000 + i64::from(object.delete_by.nanos / 1_000_000),
        Ordering::SeqCst,
    );
    assert_kind(read.await, Kind::Expired)?;
    let id = &a.data().capture.scope.request;
    assert_kind(
        store
            .delete(id, &object, &CleanupOwner(false, object.clone()))
            .await,
        Kind::Denied,
    )?;
    store
        .delete(id, &object, &CleanupOwner(true, object.clone()))
        .await?;
    store
        .delete(id, &object, &CleanupOwner(true, object.clone()))
        .await?;
    assert!(memory.head(&key(&object)).await.is_err());
    Ok(())
}
