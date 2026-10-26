//! Pure synthetic capture/codec proofs; no provider, storage or implicit readiness.
#[allow(dead_code)]
#[path = "support/segment.rs"]
mod fixture;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use fixture::*;

#[test]
fn stream_segment_freezing_binds_session_scope_epoch_generation_and_retention() -> Result {
    let f = Fixture::new(AT, 1)?;
    let s = f.session(1)?;
    let c = connection(&s)?;
    let o = observation(&f, 10, 0)?;
    let records = [CapturedStreamObservation {
        frame: 2,
        item: 4,
        observation: &o,
    }];
    let now = at(AT + 100)?;
    let freeze =
        |input| freeze_stream_segment(&s, &c, f.catalogs(), now, &Cancellation::default(), input);
    // Explicit time remains fixed; no wall-clock or vendor input participates.
    let a = freeze(input(&f, &records)?)?;
    assert_eq!(a, segment()?);
    assert_eq!(
        a.data().scope.session_sha256,
        DailyEvidenceArtifact::hash(s.bytes())
    );
    assert_eq!(a.data().generation.as_str(), "7");
    assert_eq!(
        a.data()
            .records
            .first()
            .ok_or("record")?
            .sequence
            .as_ref()
            .ok_or("sequence")?
            .as_str(),
        "0"
    );
    let mut changed = input(&f, &records)?;
    changed.epoch = edgeagent_market_data_domain::FeedEpoch::new(4)?;
    assert_ne!(freeze(changed)?.digest(), a.digest());
    let mut changed = input(&f, &records)?;
    changed.delete_by = at(AT + 86_400_001)?;
    assert!(freeze(changed).is_err());
    let mut changed = input(&f, &records)?;
    changed.frames = [1, 4];
    assert!(freeze(changed).is_err());
    let mut changed = input(&f, &records)?;
    changed.instrument = edgeagent_market_data_domain::InstrumentId::new(999)?;
    assert!(freeze(changed).is_err());
    let other = f.session(2)?;
    assert!(
        freeze_stream_segment(
            &other,
            &c,
            f.catalogs(),
            at(AT + 100)?,
            &Cancellation::default(),
            input(&f, &records)?
        )
        .is_err()
    );
    let revoked = f.revoked()?;
    assert!(
        freeze_stream_segment(
            &s,
            &c,
            AuthorityCatalogs {
                rights: &revoked,
                providers: &f.providers
            },
            at(AT + 100)?,
            &Cancellation::default(),
            input(&f, &records)?
        )
        .is_err()
    );
    let cancelled = Cancellation::default();
    cancelled.cancel();
    assert!(
        freeze_stream_segment(
            &s,
            &c,
            f.catalogs(),
            at(AT + 100)?,
            &cancelled,
            input(&f, &records)?
        )
        .is_err()
    );
    let empty = freeze(input(&f, &[])?)?;
    assert!(empty.data().records.is_empty()); // Control capture is not empty accepted evidence.
    // A short licensed lifetime is anchored to receipt, not the later sealing time.
    let mut tight = Fixture::new(AT, 1)?;
    let grants = tight
        .rights
        .at(at(AT)?)
        .map(|old| {
            let mut grant = *old.value();
            grant.canonical_retention_ms = session_fixture::nz(500)?;
            Ok(edgeagent_market_data_domain::ReferenceVersion::new(
                old.id(),
                old.revision(),
                old.source(),
                old.known_at(),
                *old.checksum(),
                grant,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    tight.rights = edgeagent_market_data_domain::ReferenceHistory::new(grants)?;
    let tight_session = tight.session(1)?;
    let tight_connection = connection(&tight_session)?;
    let mut too_late = input(&tight, &records)?;
    too_late.delete_by = at(AT + 600)?;
    assert!(
        freeze_stream_segment(
            &tight_session,
            &tight_connection,
            tight.catalogs(),
            now,
            &Cancellation::default(),
            too_late
        )
        .is_err()
    );
    let mut exact = input(&tight, &records)?;
    exact.delete_by = at(AT + 501)?;
    freeze_stream_segment(
        &tight_session,
        &tight_connection,
        tight.catalogs(),
        now,
        &Cancellation::default(),
        exact,
    )?;
    Ok(())
}
#[test]
fn stream_segment_codec_rejects_noncanonical_corrupt_and_unbounded_input() -> Result {
    let a = segment()?;
    // Independent invented v1 byte floor. Do not refresh to hide incompatible format drift.
    assert_eq!(
        a.digest().as_str(),
        "0cc7601ef9aef32ed060c0f07c6e52f18356f820aa1c28aeaaacfd007efda674"
    );
    assert_eq!(StreamSegment::decode(a.bytes(), a.digest())?, a);
    assert!(DailyEvidenceArtifact::decode(a.bytes(), a.digest()).is_err());
    let mutate = |field: &str, value: serde_json::Value| -> Result {
        let mut json: serde_json::Value = serde_json::from_slice(a.bytes())?;
        json.as_object_mut()
            .ok_or("object")?
            .insert(field.into(), value);
        let bytes = serde_json::to_vec(&json)?;
        assert!(StreamSegment::decode(&bytes, &DailyEvidenceArtifact::hash(&bytes)).is_err());
        Ok(())
    };
    mutate("version", serde_json::json!("edgeagent.evidence.daily.v1"))?;
    mutate("unknown", serde_json::json!("SYNTHETIC_PRIVATE_SENTINEL"))?;
    let mut alternate = a.bytes().to_vec();
    alternate.push(b' ');
    assert!(StreamSegment::decode(&alternate, &DailyEvidenceArtifact::hash(&alternate)).is_err());
    assert!(StreamSegment::decode(a.bytes(), &DailyEvidenceArtifact::hash(b"wrong")).is_err());
    let oversized = vec![b' '; MAX_STREAM_SEGMENT_BYTES + 1];
    assert!(StreamSegment::decode(&oversized, &DailyEvidenceArtifact::hash(&oversized)).is_err());
    let mut data = a.data().clone();
    data.limits.records = 0;
    assert!(StreamSegment::new(data).is_err());
    let mut data = a.data().clone();
    data.limits.bytes = 1;
    assert!(StreamSegment::new(data).is_err());
    let mut data = a.data().clone();
    data.records = vec![data.records.first().ok_or("record")?.clone(); 4097];
    assert!(StreamSegment::new(data).is_err());
    let text = std::str::from_utf8(a.bytes())?;
    let duplicate = text.replacen(
        "\"version\":",
        "\"version\":\"edgeagent.evidence.stream-segment.v1\",\"version\":",
        1,
    );
    assert!(
        StreamSegment::decode(
            duplicate.as_bytes(),
            &DailyEvidenceArtifact::hash(duplicate.as_bytes())
        )
        .is_err()
    );
    let e = StreamSegment::decode(
        b"SYNTHETIC_PRIVATE_SENTINEL",
        &DailyEvidenceArtifact::hash(b"SYNTHETIC_PRIVATE_SENTINEL"),
    )
    .err()
    .ok_or("error")?;
    assert!(!format!("{e:?} {e} {a:?} {:?}", a.object()).contains("SYNTHETIC_PRIVATE_SENTINEL"));
    Ok(())
}
#[test]
fn stream_segment_values_preserve_cancellations_and_source_semantics() -> Result {
    let a = segment()?;
    let decimal = |s: &str| EvidenceDecimal {
        coefficient: s.into(),
        scale: 0,
    };
    let record = a.data().records.first().ok_or("record")?;
    let variants = [
        StreamValue::Index {
            points: decimal("1000"),
        },
        StreamValue::Bar {
            coverage: StreamBarCoverage::Intraday,
            bucket: [
                EvidenceTime {
                    seconds: record.event_at.seconds - 1,
                    nanos: 0,
                },
                record.event_at,
            ],
            ohlc: [decimal("2"), decimal("3"), decimal("1"), decimal("2")],
            volume: decimal("0"),
        },
        StreamValue::OptionQuote {
            specification: a.data().semantics.clone(),
            bid: StreamQuoteSide {
                points: decimal("0"),
                size: None,
                at: record.event_at,
            },
            ask: StreamQuoteSide {
                points: decimal("1"),
                size: Some(EvidenceSequence::new("0")?),
                at: record.event_at,
            },
        },
    ];
    for value in variants {
        let mut data = a.data().clone();
        data.records.first_mut().ok_or("record")?.value = Some(value);
        data.records.first_mut().ok_or("record")?.sequence =
            Some(EvidenceSequence::new("18446744073709551615")?);
        let sealed = StreamSegment::new(data)?;
        assert_eq!(
            StreamSegment::decode(sealed.bytes(), sealed.digest())?,
            sealed
        );
    }
    let mut data = a.data().clone();
    let r = data.records.first_mut().ok_or("record")?;
    r.observation.revision = EvidenceRevision::new("2")?;
    r.predecessor = Some(EvidenceRevision::new("1")?);
    r.value = None;
    let cancelled = StreamSegment::new(data.clone())?;
    assert!(
        StreamSegment::decode(cancelled.bytes(), cancelled.digest())?
            .data()
            .records
            .first()
            .ok_or("record")?
            .value
            .is_none()
    );
    data.records.first_mut().ok_or("record")?.predecessor = None;
    assert!(StreamSegment::new(data).is_err());
    let mut data = a.data().clone();
    data.records.first_mut().ok_or("record")?.sequence = None;
    assert!(StreamSegment::new(data.clone()).is_err());
    data.delivery = StreamDelivery::Snapshots;
    StreamSegment::new(data)?;
    let mut data = a.data().clone();
    data.records
        .push(data.records.first().ok_or("record")?.clone());
    assert!(StreamSegment::new(data).is_err());
    let mut data = a.data().clone();
    data.records.first_mut().ok_or("record")?.value = Some(StreamValue::Trade {
        price: EvidenceDecimal {
            coefficient: "123450".into(),
            scale: 3,
        },
        shares: EvidenceDecimal {
            coefficient: "0".into(),
            scale: 0,
        },
    });
    assert!(StreamSegment::new(data).is_err());
    // Repeated source facts at distinct local arrivals are retained, not silently deduplicated.
    let mut data = a.data().clone();
    let mut duplicate = data.records.first().ok_or("record")?.clone();
    duplicate.frame = EvidenceRevision::new("3")?;
    data.records.push(duplicate);
    let repeated = StreamSegment::new(data.clone())?;
    assert_eq!(repeated.data().records.len(), 2);
    data.records.last_mut().ok_or("record")?.conditions = vec![2];
    assert!(StreamSegment::new(data).is_err());
    Ok(())
}
