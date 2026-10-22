mod support;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use support::*;

#[test]
fn rights_permissions_scope_and_conflicts_fail_closed() -> Result {
    let f = Fixture::new()?;
    let cancel = Cancellation::default();
    assert!(f.catalogs().admit(&f.request, now()?, &cancel).is_ok());
    for permission in [Permission::Unknown, Permission::Denied] {
        for field in 0..4 {
            let mut f = Fixture::new()?;
            let mut r = f.right;
            match field {
                0 => r.permissions.automated = permission,
                1 => r.permissions.derived = permission,
                2 => r.permissions.canonical_storage = permission,
                _ => r.permissions.audience = permission,
            }
            f.rights = ReferenceHistory::new(vec![version(1, 1, at(0)?, r)?])?;
            assert_eq!(
                f.catalogs().admit(&f.request, now()?, &cancel).err(),
                Some(AcquisitionError::Rights)
            );
        }
    }
    for field in 0..7 {
        let mut f = Fixture::new()?;
        match field {
            0 => f.request.scope.principal = PrincipalId::new(2)?,
            1 => f.request.scope.account = ProviderAccountId::new(2)?,
            2 => f.request.scope.source = SourceId::new(2)?,
            3 => f.request.scope.instrument = InstrumentId::new(2)?,
            4 => f.request.scope.endpoint = ReferenceId::new(2)?,
            5 => f.request.scope.audience = AudienceId::new(2)?,
            _ => f.request.scope.storage = StorageScopeId::new(2)?,
        }
        assert_eq!(
            f.catalogs().admit(&f.request, now()?, &cancel).err(),
            Some(AcquisitionError::Rights)
        );
    }
    let mut f = Fixture::new()?;
    f.rights = ReferenceHistory::new(vec![
        version(1, 1, at(0)?, f.right)?,
        version(2, 1, at(0)?, f.right)?,
    ])?;
    assert_eq!(
        f.catalogs().admit(&f.request, now()?, &cancel).err(),
        Some(AcquisitionError::Rights)
    );
    Ok(())
}

#[test]
fn latest_authority_revisions_never_resurrect_withdrawn_or_future_records() -> Result {
    let mut f = Fixture::new()?;
    let cancel = Cancellation::default();
    let mut withdrawn = f.right;
    withdrawn.status = ReviewStatus::Revoked;
    f.rights = ReferenceHistory::new(vec![
        version(1, 1, at(0)?, f.right)?,
        version(1, 2, at(NOW + 1)?, withdrawn)?,
    ])?;
    assert!(f.catalogs().admit(&f.request, now()?, &cancel).is_ok());
    assert_eq!(
        f.catalogs().admit(&f.request, at(NOW + 1)?, &cancel).err(),
        Some(AcquisitionError::Rights)
    );
    withdrawn = f.right;
    withdrawn.scope.instrument = InstrumentId::new(2)?;
    f.rights = ReferenceHistory::new(vec![
        version(1, 1, at(0)?, f.right)?,
        version(1, 2, now()?, withdrawn)?,
    ])?;
    assert_eq!(
        f.catalogs().admit(&f.request, now()?, &cancel).err(),
        Some(AcquisitionError::Rights)
    );
    f.rights = ReferenceHistory::new(vec![version(1, 1, at(NOW + 1)?, f.right)?])?;
    assert_eq!(
        f.catalogs().admit(&f.request, now()?, &cancel).err(),
        Some(AcquisitionError::Rights)
    );
    Ok(())
}

#[test]
fn provider_capabilities_and_historical_knowledge_require_review() -> Result {
    for mode in 0..3 {
        let mut f = Fixture::new()?;
        let mut review = f.review;
        match mode {
            0 => review.effective = EffectiveInterval::new(at(0)?, now()?)?,
            1 => {
                review.capability = ProviderCapability::new(
                    SourceId::new(2)?,
                    f.request.contract,
                    DeliveryCapability::Snapshots,
                    KnowledgeCapability::CaptureOnly,
                )
            }
            _ => review.scope.endpoint = ReferenceId::new(2)?,
        }
        f.providers = ReferenceHistory::new(vec![version(1, 1, at(0)?, review)?])?;
        assert_eq!(
            f.catalogs()
                .admit(&f.request, now()?, &Cancellation::default())
                .err(),
            Some(AcquisitionError::Conformance)
        );
    }
    let mut f = Fixture::new()?;
    let cancel = Cancellation::default();
    f.providers = ReferenceHistory::new(vec![])?;
    assert_eq!(
        f.catalogs().admit(&f.request, now()?, &cancel).err(),
        Some(AcquisitionError::Conformance)
    );
    f = Fixture::new()?;
    f.request.knowledge = KnowledgeMode::RetainedAt(at(DAY)?);
    assert_eq!(
        f.catalogs().admit(&f.request, now()?, &cancel).err(),
        Some(AcquisitionError::Conformance)
    );
    f.review.capability = ProviderCapability::new(
        f.request.scope.source,
        f.request.contract,
        DeliveryCapability::Snapshots,
        KnowledgeCapability::HistoricalVersions,
    );
    f.providers = ReferenceHistory::new(vec![version(1, 1, at(0)?, f.review)?])?;
    let token = f.catalogs().admit(&f.request, now()?, &cancel)?;
    let batch = f.batch(at(DAY)?)?;
    validate_batch(&token, f.catalogs(), now()?, &cancel, &batch)?;
    assert_eq!(
        validate_batch(&token, f.catalogs(), now()?, &cancel, &f.batch(now()?)?),
        Err(AcquisitionError::Response)
    );
    let mut review = f.review;
    review.status = ReviewStatus::Revoked;
    let revoked = ReferenceHistory::new(vec![
        version(1, 1, at(0)?, f.review)?,
        version(1, 2, now()?, review)?,
    ])?;
    assert_eq!(
        validate_batch(
            &token,
            AuthorityCatalogs {
                rights: &f.rights,
                providers: &revoked
            },
            now()?,
            &cancel,
            &batch
        ),
        Err(AcquisitionError::Conformance)
    );
    Ok(())
}

#[test]
fn bounded_requests_cancellation_deadlines_and_retention_are_explicit() -> Result {
    let mut long = Fixture::new()?;
    long.request.range =
        DateRange::new(SessionDate::new(1970, 1, 1)?, SessionDate::new(1971, 1, 3)?)?;
    assert_eq!(
        long.catalogs()
            .admit(&long.request, now()?, &Cancellation::default())
            .err(),
        Some(AcquisitionError::InvalidRequest)
    );
    let mut f = Fixture::new()?;
    let cancel = Cancellation::default();
    let token = f.catalogs().admit(&f.request, now()?, &cancel)?;
    assert_eq!(token.delete_by(), at(NOW + DAY)?);
    assert!(token.raw_delete_by().is_none());
    assert_eq!(
        f.catalogs()
            .admit(&f.request, f.request.deadline, &cancel)
            .err(),
        Some(AcquisitionError::Deadline)
    );
    for field in 0..7 {
        let mut request = f.request;
        match field {
            0 => request.budget.pages = nz(5)?,
            1 => request.budget.calls = nz(13)?,
            2 => request.budget.connect_ms = nz(5001)?,
            3 => request.budget.request_ms = nz(15001)?,
            4 => request.budget.response_bytes = nz(1_048_577)?,
            5 => request.budget.command_bytes = nz(8_388_609)?,
            _ => request.deadline = at(NOW + 300_001)?,
        }
        assert_eq!(
            f.catalogs().admit(&request, now()?, &cancel).err(),
            Some(AcquisitionError::InvalidRequest)
        );
    }
    f.request.raw = RawRetention::Retain;
    assert_eq!(
        f.catalogs().admit(&f.request, now()?, &cancel).err(),
        Some(AcquisitionError::Rights)
    );
    f.right.raw_storage = Permission::Allowed;
    f.right.raw_retention_ms = Some(nz(1000)?);
    f.right.delete_by = at(NOW + 500)?;
    f.rights = ReferenceHistory::new(vec![version(1, 1, at(0)?, f.right)?])?;
    let token = f.catalogs().admit(&f.request, now()?, &cancel)?;
    assert_eq!(token.delete_by(), at(NOW + 500)?);
    assert_eq!(token.raw_delete_by(), Some(token.delete_by()));
    assert_eq!(
        validate_batch(
            &token,
            f.catalogs(),
            at(NOW + 500)?,
            &cancel,
            &f.batch(now()?)?
        ),
        Err(AcquisitionError::Rights)
    );
    cancel.cancel();
    assert_eq!(
        f.catalogs().admit(&f.request, now()?, &cancel).err(),
        Some(AcquisitionError::Cancelled)
    );
    Ok(())
}

#[test]
fn provider_results_cannot_expand_authority_or_bypass_provenance() -> Result {
    let f = Fixture::new()?;
    let cancel = Cancellation::default();
    let token = f.catalogs().admit(&f.request, now()?, &cancel)?;
    for field in 0..4 {
        let mut batch = f.batch(now()?)?;
        match field {
            0 => batch.request = AcquisitionId::new(2)?,
            1 => batch.observations.clear(),
            2 => batch.observations = vec![batch.observations[0].clone(); 4097],
            _ => batch = f.batch(at(NOW + 1)?)?,
        }
        assert_eq!(
            validate_batch(&token, f.catalogs(), now()?, &cancel, &batch),
            Err(AcquisitionError::Response)
        );
    }
    let mut f = Fixture::new()?;
    f.right.embargo_ms = u32::try_from(2 * DAY)?;
    f.rights = ReferenceHistory::new(vec![version(1, 1, at(0)?, f.right)?])?;
    let token = f.catalogs().admit(&f.request, now()?, &cancel)?;
    assert_eq!(
        validate_batch(&token, f.catalogs(), now()?, &cancel, &f.batch(now()?)?),
        Err(AcquisitionError::Rights)
    );
    Ok(())
}

#[test]
fn synthetic_provider_port_requires_admission_and_post_call_authority() -> Result {
    let f = Fixture::new()?;
    let cancel = Cancellation::default();
    let token = f.catalogs().admit(&f.request, now()?, &cancel)?;
    let fake = Fake {
        batch: f.batch(now()?)?,
        calls: std::sync::atomic::AtomicUsize::new(0),
    };
    let batch = ready(fake.fetch(&token, &cancel))??;
    assert!(!format!("{batch:?}").contains("123456789"));
    validate_batch(&token, f.catalogs(), now()?, &cancel, &batch)?;
    assert_eq!(fake.calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    let mut withdrawn = f.right;
    withdrawn.status = ReviewStatus::Revoked;
    let rights = ReferenceHistory::new(vec![
        version(1, 1, at(0)?, f.right)?,
        version(1, 2, now()?, withdrawn)?,
    ])?;
    assert_eq!(
        validate_batch(
            &token,
            AuthorityCatalogs {
                rights: &rights,
                providers: &f.providers
            },
            now()?,
            &cancel,
            &batch
        ),
        Err(AcquisitionError::Rights)
    );
    cancel.cancel();
    assert_eq!(
        validate_batch(&token, f.catalogs(), now()?, &cancel, &batch),
        Err(AcquisitionError::Cancelled)
    );
    assert_eq!(
        ready(fake.fetch(&token, &cancel))?,
        Err(ProviderFailure::Cancelled)
    );
    Ok(())
}
