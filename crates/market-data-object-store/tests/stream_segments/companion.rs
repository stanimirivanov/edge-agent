use super::*;
fn native_key(o: &StreamCompanionObject) -> Path {
    let s = &o.segment.scope;
    Path::from(format!(
        "evidence/stream-companion-v1/{}/{}/{}/{}/{}/{}.json",
        s.storage.as_str(),
        s.audience.as_str(),
        s.rights.id.as_str(),
        s.rights.revision.as_str(),
        s.session.as_str(),
        o.sha256.as_str()
    ))
}
#[tokio::test(start_paused = true)]
async fn native_companion_store_verifies_uncertain_creation_denial_and_paired_cleanup() -> Result {
    let backend = Arc::new(InMemory::new());
    let slow = Arc::new(ThrottledStore::new(
        backend.clone(),
        ThrottleConfig::default(),
    ));
    slow.config_mut(|c| c.wait_get_per_call = Duration::from_secs(2));
    let segment = fixture::segment()?;
    let c = fixture::companion(&segment)?;
    let o = c.object();
    let access = Access::new(segment.object());
    kind(adapter(slow)?.put_companion(&c, &access).await, K::Deadline)?;
    assert!(backend.head(&native_key(&o)).await.is_ok());
    let store = adapter(backend.clone())?;
    store.put_segment(&segment, &access).await?;
    assert_eq!(store.put_companion(&c, &access).await?, o);
    assert_eq!(store.get_companion(&o, &access).await?, c);
    access.allowed.store(false, Ordering::SeqCst);
    kind(store.get_companion(&o, &access).await, K::Denied)?;
    access.allowed.store(true, Ordering::SeqCst);
    access.now.store(AT + 1000, Ordering::SeqCst);
    kind(store.get_companion(&o, &access).await, K::Expired)?;
    access.now.store(AT, Ordering::SeqCst);
    let mut wrong = o.clone();
    wrong.bytes += 1;
    kind(store.get_companion(&wrong, &access).await, K::Integrity)?;
    let mut wrong_access = Access::new(segment.object());
    wrong_access.deny_after = 1;
    kind(store.get_companion(&o, &wrong_access).await, K::Denied)?;
    let cleanup = StreamCleanup {
        publication: EvidenceId::new(&format!("{:032x}", 1))?,
        object: segment.object(),
        companion: Some(o.clone()),
    };
    kind(
        store.delete_segment(&cleanup, &WriterQuiet(false)).await,
        K::Denied,
    )?;
    store.delete_segment(&cleanup, &WriterQuiet(true)).await?;
    store.delete_segment(&cleanup, &WriterQuiet(true)).await?;
    kind(store.get_companion(&o, &access).await, K::NotFound)?;
    kind(
        store.get_segment(&segment.object(), &access).await,
        K::NotFound,
    )?;
    // A collision is never repaired or compensated by deleting its canonical partner.
    store.put_segment(&segment, &access).await?;
    backend
        .put(&native_key(&o), vec![b'!'; o.bytes as usize].into())
        .await?;
    kind(store.put_companion(&c, &access).await, K::Integrity)?;
    assert!(store.get_segment(&segment.object(), &access).await.is_ok());
    Ok(())
}
