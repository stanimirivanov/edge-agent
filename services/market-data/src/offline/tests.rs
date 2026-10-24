use super::*;

#[test]
fn offline_mode_rejects_vendor_and_live_flags_before_infrastructure() {
    for flag in [None, Some("1"), Some("true")] {
        assert!(config::mode(flag, None).is_ok());
    }
    for flag in ["0", "false", "TRUE", ""] {
        assert!(config::mode(Some(flag), None).is_err());
    }
    assert!(config::mode(Some("1"), Some("vendor")).is_err());
    assert!(config::local_endpoint("postgresql://127.0.0.1/offline", &["postgresql"]).is_ok());
    for url in [
        "postgresql://remote.invalid/offline",
        "postgresql://127.0.0.1/offline?host=remote.invalid",
        "postgresql://127.0.0.1/offline#override",
    ] {
        assert!(config::local_endpoint(url, &["postgresql"]).is_err());
    }
}

#[tokio::test]
async fn compiled_profile_is_pinned_and_cancellation_cannot_activate_a_source() -> Result {
    let time = 1_792_800_000_000;
    let a = Job::new(1, time)?;
    let b = Job::new(1, time)?;
    assert_eq!(a.record, b.record);
    assert!(a.record.configuration().is_some());
    assert_ne!(a.record, Job::new(2, time)?.record);
    let cancel = Cancellation::default();
    let token = a
        .catalogs()
        .admit(a.record.request(), a.record.request().admitted_at, &cancel)?;
    let first = a.source.fetch(&token, &cancel).await?;
    let second = a.source.fetch(&token, &cancel).await?;
    assert_eq!(first, second);
    cancel.cancel();
    assert!(a.source.fetch(&token, &cancel).await.is_err());
    Ok(())
}

#[test]
fn cleanup_never_infers_account_or_write_quiescence() -> Result {
    let request = EvidenceId::new("00000000000000000000000000000001")?;
    let confirmed = BTreeSet::new();
    let policy = Cleanup {
        confirmed: &confirmed,
    };
    assert!(policy.confirm_writes_quiescent(&request).is_err());
    assert!(policy.confirm_account_quiescent(&request, 1).is_err());
    let confirmed = BTreeSet::from([request.clone()]);
    let policy = Cleanup {
        confirmed: &confirmed,
    };
    assert!(policy.confirm_writes_quiescent(&request).is_ok());
    assert!(policy.confirm_account_quiescent(&request, 1).is_err());
    Ok(())
}

#[tokio::test]
async fn compiled_profile_passes_quality_and_creates_a_scoped_artifact() -> Result {
    let job = Job::new(1, now()?.unix_millis())?;
    let storage = ImmutableEvidenceStore::new(
        Arc::new(object_store::memory::InMemory::new()),
        Duration::from_secs(15),
    )?;
    let access = Access {
        job: &job,
        captured: now()?,
    };
    if let Err(error) = job
        .prepare(&storage, &access, &Cancellation::default())
        .await
    {
        if let Some(freeze) = error
            .source()
            .and_then(|e| e.downcast_ref::<EvidenceFreezeError>())
        {
            eprintln!("freeze rejection: {freeze:?}");
        }
        return Err(error);
    }
    Ok(())
}

#[test]
fn profile_lock_excludes_a_second_owner_and_refuses_reinitialization() -> Result {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!("edgeagent-lock-{}-{unique}", std::process::id()));
    std::fs::create_dir(&root)?;
    let config = config::Config {
        command: "offline-init".into(),
        database: String::new(),
        root: root.clone(),
        request: None,
        broker: None,
        stream: None,
        consumer: None,
        messages: 0,
        confirmed: BTreeSet::new(),
    };
    let result = (|| -> Result {
        let owner = config.lock()?;
        assert!(config.lock().is_err());
        let schema = config.schema()?;
        assert!(schema.starts_with("edgeagent_offline_"));
        assert!(schema[18..].bytes().all(|v| v.is_ascii_hexdigit()));
        drop(owner);
        std::fs::create_dir(root.join("objects"))?;
        assert!(config.lock().is_err());
        Ok(())
    })();
    // This test owns this freshly created exact directory, including the lock file.
    std::fs::remove_dir_all(&root)?;
    result
}
