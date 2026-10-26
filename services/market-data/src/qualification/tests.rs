use super::*;
use serde_json::{Value, json};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn fixture(
    provider: QualificationProvider,
) -> TestResult<(AdmissionRecord, QualificationProfile, Value)> {
    let at = |ms| UtcInstant::from_unix_millis(ms);
    let eid = |n: u128| format!("{n:032x}");
    let reference = |n: u128, value: Value| json!([eid(n), 1, eid(1), 0, "07".repeat(32), value]);
    let admitted = at(172_800_000)?;
    let end = 173_400_000;
    let dates = json!(["1970-01-01", "1970-01-02"]);
    let effective = EffectiveInterval::new(at(0)?, at(end)?)?;
    let scope = AccessScope {
        principal: PrincipalId::new(1)?,
        account: ProviderAccountId::new(1)?,
        source: SourceId::new(1)?,
        instrument: InstrumentId::new(1)?,
        endpoint: ReferenceId::new(1)?,
        audience: AudienceId::new(1)?,
        storage: StorageScopeId::new(1)?,
    };
    let precision = match provider {
        QualificationProvider::TwelveDataDaily => TimestampPrecision::DateOnly,
        QualificationProvider::AlpacaHistoricalSipDaily => TimestampPrecision::Nanoseconds,
    };
    let contract = EvidenceContract::new(
        EvidenceKind::RawBar(BarCoverage::ProviderDay),
        MarketScope::Consolidated,
        SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
        precision,
    )?;
    let r = AcquisitionRequest {
        id: AcquisitionId::new(1)?,
        scope,
        contract,
        range: DateRange::new(SessionDate::new(1970, 1, 1)?, SessionDate::new(1970, 1, 2)?)?,
        knowledge: KnowledgeMode::Capture,
        admitted_at: admitted,
        deadline: at(173_100_000)?,
        raw: RawRetention::Discard,
        budget: RequestBudget {
            connect_ms: std::num::NonZeroU32::new(1000).ok_or("bound")?,
            request_ms: std::num::NonZeroU32::new(1000).ok_or("bound")?,
            calls: std::num::NonZeroU32::new(4).ok_or("bound")?,
            pages: std::num::NonZeroU32::new(4).ok_or("bound")?,
            response_bytes: std::num::NonZeroU32::new(8192).ok_or("bound")?,
            command_bytes: std::num::NonZeroU32::new(32768).ok_or("bound")?,
        },
    };
    let w = |n, value| -> TestResult<_> {
        Ok(ReferenceVersion::new(
            ReferenceId::new(n)?,
            ReferenceRevision::new(1)?,
            SourceId::new(1)?,
            at(0)?,
            [7; 32],
            value,
        ))
    };
    let right = RightsRecord {
        scope,
        contract,
        agreement: ReferenceId::new(3)?,
        effective,
        permissions: UsePermissions {
            automated: Permission::Allowed,
            derived: Permission::Allowed,
            canonical_storage: Permission::Allowed,
            audience: Permission::Allowed,
        },
        raw_storage: Permission::Denied,
        canonical_retention_ms: std::num::NonZeroU32::new(600_000).ok_or("bound")?,
        raw_retention_ms: None,
        delete_by: at(end)?,
        embargo_ms: 900_000,
        status: ReviewStatus::Active,
    };
    let rights = ReferenceHistory::new(vec![w(1, right)?])?;
    let review = ProviderReview {
        scope,
        capability: ProviderCapability::new(
            scope.source,
            contract,
            DeliveryCapability::Snapshots,
            KnowledgeCapability::CaptureOnly,
        ),
        effective,
        status: ReviewStatus::Active,
    };
    let providers = ReferenceHistory::new(vec![ReferenceVersion::new(
        ReferenceId::new(2)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        at(0)?,
        [7; 32],
        review,
    )])?;
    let mut value = json!({
        "version":"edgeagent.qualification-input.v1",
        "rights":[reference(1,json!([eid(3),[0,end],["allowed","allowed","allowed","allowed"],"denied",600000,null,end,900000,"active"]))],
        "providers":[reference(2,json!([[0,end],"snapshots","capture_only","active"]))],
        "mappings":[reference(4,json!([eid(1),eid(1),"INVENTED","XNYS",dates.clone()]))],
        "calendars":[reference(5,json!(["XNYS","1970-01-01","regular",[3600000,7200000],"invented-v1"]))],
        "actions":[],"coverage":reference(7,json!([eid(1),dates.clone(),end])),
        "quality":[1,259200000,1000,100],"transformation":1,"observations":[[eid(10),1,null]],
        "context":[eid(1),1,172800000,172800000,"1970-01-01"],
        "selections":[reference(8,json!([[0,end],[eid(1)]]))],
        "directories":[reference(9,json!([[0,end],[reference(4,json!([eid(1),eid(1),"INVENTED","XNYS",dates]))]]))],
        "selection_ids":[eid(8),eid(9)],"buckets":[]
    });
    let mapping = ReferenceVersion::new(
        ReferenceId::new(4)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        at(0)?,
        [7; 32],
        InstrumentMapping::new(
            scope.instrument,
            ListingId::new(1)?,
            Symbol::new("INVENTED")?,
            Venue::Xnys,
            r.range,
        ),
    );
    let calendar = ReferenceVersion::new(
        ReferenceId::new(5)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        at(0)?,
        [7; 32],
        CalendarDay::new(
            Venue::Xnys,
            SessionDate::new(1970, 1, 1)?,
            SessionHours::Regular(SessionWindow::new(at(3600000)?, at(7200000)?)?),
            "invented-v1",
        )?,
    );
    let routing = RoutingScope {
        source: scope.source,
        feed: FeedId::new(1)?,
        epoch: FeedEpoch::new(1)?,
    };
    let selections = ReferenceHistory::new(vec![ReferenceVersion::new(
        ReferenceId::new(8)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        at(0)?,
        [7; 32],
        InstrumentSelection::new(effective, &[scope.instrument])?,
    )])?;
    let directories = ReferenceHistory::new(vec![ReferenceVersion::new(
        ReferenceId::new(9)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        at(0)?,
        [7; 32],
        RoutingDirectory::new(
            routing,
            effective,
            vec![RouteBinding::new(
                RoutingKey::symbol(&Symbol::new("INVENTED")?),
                mapping,
            )?],
        )?,
    )])?;
    let plan = SelectionPlan::resolve(
        SelectionContext {
            scope: routing,
            effective_at: admitted,
            cutoff: admitted,
            session: SessionDate::new(1970, 1, 1)?,
        },
        (ReferenceId::new(8)?, ReferenceId::new(9)?),
        &selections,
        &directories,
    )?;
    let selected = plan.prepare(&r)?;
    let coverage = EvidenceInterval::new(
        PreciseInstant::from_millis(at(0)?),
        PreciseInstant::from_millis(at(86400000)?),
    )?;
    let digest = match provider {
        QualificationProvider::TwelveDataDaily => {
            value["buckets"] = json!([reference(
                6,
                json!(["1970-01-01", [[0, 0], [86400, 0]], eid(5), 1])
            )]);
            DateOnlySchedule::new(
                &selected,
                &[calendar],
                &[ReferenceVersion::new(
                    ReferenceId::new(6)?,
                    ReferenceRevision::new(1)?,
                    SourceId::new(1)?,
                    at(0)?,
                    [7; 32],
                    DateOnlyBucket::new(
                        SessionDate::new(1970, 1, 1)?,
                        coverage,
                        SemanticReference::new(ReferenceId::new(5)?, ReferenceRevision::new(1)?),
                    )?,
                )],
            )?
            .configuration_digest()
            .clone()
        }
        QualificationProvider::AlpacaHistoricalSipDaily => {
            value["buckets"] = json!([["1970-01-01", [[0, 0], [86400, 0]], [eid(10), 1, null]]]);
            PreciseDailySchedule::new(
                &selected,
                &[calendar],
                &[DailyObservationSlot {
                    date: SessionDate::new(1970, 1, 1)?,
                    bucket: coverage,
                    version: ObservationVersion::new(
                        ObservationId::new(10)?,
                        ObservationRevision::new(1)?,
                        None,
                    )?,
                }],
                TransformationRevision::new(1)?,
            )?
            .configuration_digest()
            .clone()
        }
    };
    let auth = AuthorityCatalogs {
        rights: &rights,
        providers: &providers,
    }
    .admit(&r, admitted, &Cancellation::default())?;
    let record = AdmissionRecord::new(
        &auth,
        AcquisitionLimits {
            attempts: 3,
            spacing_ms: 15000,
            starts_per_minute: 4,
        },
        ListingId::new(1)?,
        DailyQualityPolicy::new(TransformationRevision::new(1)?, 259200000, 1000, 100)?,
    )?
    .with_configuration(&digest)?;
    let p = QualificationProfile::new(QualificationProfileData {
        version: QualificationProfileFormat::V1,
        provider,
        admission: record.command().clone(),
        configuration_sha256: digest,
        owner_review: EvidenceVersion {
            id: EvidenceId::new(&eid(90))?,
            revision: EvidenceRevision::new("1")?,
        },
        credits: DailyCreditPolicy::new(1, 100, end, &[(scope.endpoint, 1)])?,
        not_before: EvidenceTime {
            seconds: 172800,
            nanos: 0,
        },
        expires_at: EvidenceTime {
            seconds: 173100,
            nanos: 0,
        },
    })?;
    Ok((record, p, value))
}

#[test]
fn both_complete_owner_inputs_round_trip_and_reject_changed_or_unknown_values() -> TestResult {
    for provider in [
        QualificationProvider::TwelveDataDaily,
        QualificationProvider::AlpacaHistoricalSipDaily,
    ] {
        let (r, p, v) = fixture(provider)?;
        assert!(external_intent(true, p.digest().as_str(), &p).is_err());
        assert!(external_intent(false, "not-this-profile", &p).is_err());
        external_intent(false, p.digest().as_str(), &p)?;
        let bytes = input::canonical(v.clone())?;
        let decoded = input::Input::decode(&bytes, &r, &p)?;
        decoded.catalogs().admit(
            r.request(),
            r.request().admitted_at,
            &Cancellation::default(),
        )?;
        let mut changed = v.clone();
        changed["calendars"][0][5][4] = json!("changed-revision");
        assert!(input::Input::decode(&input::canonical(changed)?, &r, &p).is_err());
        let mut changed = v.clone();
        changed["quality"][1] = json!(1);
        assert!(input::Input::decode(&input::canonical(changed)?, &r, &p).is_err());
        let mut changed = v.clone();
        changed["unknown"] = json!(true);
        assert!(input::Input::decode(&input::canonical(changed)?, &r, &p).is_err());
        let mut changed = bytes.clone();
        changed.push(b' ');
        assert!(input::Input::decode(&changed, &r, &p).is_err());
        assert!(input::Input::decode(&vec![b'x'; 262145], &r, &p).is_err());
    }
    Ok(())
}
#[test]
fn owner_input_errors_redact_payload_and_profile_requires_explicit_arguments() {
    let e = invalid("sensitive-payload");
    assert!(!format!("{e:?} {e}").contains("sensitive-payload"));
    assert!(profile(&[]).is_err());
    assert!(profile(&["qualification-worker".into()]).is_err());
    assert!(database_options("postgresql://localhost/example").is_ok());
    assert!(database_options("postgresql://database.example/example?sslmode=disable").is_err());
    assert!(database_options("postgresql://database.example/example?sslmode=verify-full").is_ok());
    assert!(broker_url("nats://127.0.0.1:4222".into()).is_ok());
    assert!(broker_url("nats://broker.example:4222".into()).is_err());
    assert!(broker_url("tls://broker.example:4222".into()).is_ok());
}

#[test]
fn precise_preparation_rehydrates_original_time_and_passes_quality_again() -> TestResult {
    let (r, p, v) = fixture(QualificationProvider::AlpacaHistoricalSipDaily)?;
    let input = input::Input::decode(&input::canonical(v)?, &r, &p)?;
    let input::Schedule::Precise(schedule) = &input.schedule else {
        return Err("precise fixture".into());
    };
    let cancel = Cancellation::default();
    let at = r.request().admitted_at;
    let auth = input.catalogs().admit(r.request(), at, &cancel)?;
    let provider = auth.authority().1.value().capability;
    let rows = [PreciseDailyRow {
        event_at: schedule.slots()[0].bucket.bounds().0,
        ohlc: [UsdPrice::new(100, 0)?; 4],
        volume: ShareVolume::new(0, 0)?,
        received_at: at,
    }];
    let capture = schedule.capture(
        &r,
        provider,
        &rows,
        vec![DailyPageReceipt {
            body_sha256: DailyEvidenceArtifact::hash(b"invented"),
            received_at: at,
        }],
    )?;
    let (symbol, venue, _) = schedule.mapping().value().binding();
    let buckets = schedule
        .slots()
        .iter()
        .map(|s| (s.date, s.bucket))
        .collect::<Vec<_>>();
    let series = DailySeriesSpec {
        instrument: r.request().scope.instrument,
        listing: r.listing()?,
        symbol,
        venue,
        range: r.request().range,
        cutoff: at,
        provider,
        buckets: &buckets,
    };
    let freeze = |batch| {
        freeze_evidence(
            &auth,
            input.catalogs(),
            at,
            &cancel,
            EvidenceFreeze {
                command: r.command(),
                batch,
                series: &series,
                references: input.references(),
                policy: input.quality,
                previous: None,
            },
        )
    };
    let original = freeze(capture.batch())?;
    let replay = precise::batch(&original, &r, provider)?;
    assert_eq!(replay, *capture.batch());
    assert_eq!(precise::captured(&original)?, at);
    assert_eq!(freeze(&replay)?, original);
    Ok(())
}
