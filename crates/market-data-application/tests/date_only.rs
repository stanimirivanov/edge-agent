use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::freeze_date_only_time;
use edgeagent_market_data_domain::*;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

// Invented calendar/rule authority, including intentionally open DST test dates.
// These are arithmetic fixtures, not actual exchange schedules or provider semantics.
fn fixture(date: SessionDate, begin: i64, hours: i64, early: bool) -> Result<DateOnlyTime> {
    let at = |seconds: i64| UtcInstant::from_unix_millis(seconds * 1000);
    let interval = EvidenceInterval::new(
        PreciseInstant::from_millis(at(begin)?),
        PreciseInstant::from_millis(at(begin + hours * 3600)?),
    )?;
    let pin = SemanticReference::new(ReferenceId::new(2)?, ReferenceRevision::new(3)?);
    let rule = DateOnlyBucket::new(date, interval, pin)?;
    let received = PreciseInstant::from_unix_parts(begin + hours * 3600 + 10, 123)?;
    let known = at(begin - 100)?;
    let window = SessionWindow::new(
        at(begin + 9 * 3600)?,
        at(begin + if early { 12 } else { 16 } * 3600)?,
    )?;
    let calendar = CalendarDay::new(
        Venue::Xnys,
        date,
        if early {
            SessionHours::EarlyClose(window)
        } else {
            SessionHours::Regular(window)
        },
        "synthetic-2026a",
    )?;
    Ok(DateOnlyTime::new(
        date,
        ReferenceVersion::new(
            ReferenceId::new(1)?,
            ReferenceRevision::new(2)?,
            SourceId::new(7)?,
            known,
            [7; 32],
            rule,
        ),
        ReferenceVersion::new(
            ReferenceId::new(2)?,
            ReferenceRevision::new(3)?,
            SourceId::new(8)?,
            known,
            [8; 32],
            calendar,
        ),
        received,
        at(begin + hours * 3600 + 11)?,
    )?)
}

#[test]
fn native_dates_preserve_reviewed_dst_buckets_and_exact_local_receipts() -> Result {
    for (date, begin, hours, early) in [
        (SessionDate::new(2024, 3, 10)?, 1_710_046_800, 23, false),
        (SessionDate::new(2024, 11, 3)?, 1_730_606_400, 25, true),
    ] {
        let value = fixture(date, begin, hours, early)?;
        let evidence = freeze_date_only_time(&value)?;
        assert_eq!(evidence.data().received_at.nanos, 123);
        assert_eq!(
            evidence.data().bucket[1].seconds - evidence.data().bucket[0].seconds,
            hours * 3600
        );
        assert_eq!(
            evidence.data().rule.calendar,
            evidence.data().calendar.version
        );
        assert_eq!(
            evidence.data().calendar.date.as_ref(),
            Some(&evidence.data().source_date)
        );
        assert_eq!(evidence.data().tzdata_revision, "synthetic-2026a");
        assert_eq!(
            DateOnlyTimeEvidence::decode(evidence.bytes(), evidence.digest())?,
            evidence
        );
        let json: serde_json::Value = serde_json::from_slice(evidence.bytes())?;
        assert!(
            json.get("event_at").is_none(),
            "date-only must never manufacture source instants"
        );
        let mut changed = evidence.data().clone();
        changed.rule.version.revision = EvidenceRevision::new("3")?;
        assert_ne!(
            DateOnlyTimeEvidence::new(changed)?.digest(),
            evidence.digest()
        );
    }
    Ok(())
}

#[test]
fn date_only_construction_rejects_unreviewed_or_conflicting_calendar_and_chronology() -> Result {
    let date = SessionDate::new(2024, 3, 10)?;
    let value = fixture(date, 1_710_046_800, 23, false)?;
    let (rule, calendar) = value.provenance();
    let (received, derived) = value.local_times();
    assert!(
        DateOnlyTime::new(
            SessionDate::new(2024, 3, 11)?,
            rule.clone(),
            calendar.clone(),
            received,
            derived
        )
        .is_err()
    );
    assert!(
        DateOnlyTime::new(
            date,
            rule.clone(),
            calendar.clone(),
            rule.value().declaration().1.bounds().0,
            derived
        )
        .is_err()
    );
    assert!(
        DateOnlyTime::new(
            date,
            rule.clone(),
            calendar.clone(),
            received,
            rule.known_at()
        )
        .is_err()
    );
    let closed = ReferenceVersion::new(
        calendar.id(),
        calendar.revision(),
        calendar.source(),
        calendar.known_at(),
        *calendar.checksum(),
        CalendarDay::new(Venue::Xnys, date, SessionHours::Closed, "synthetic-2026a")?,
    );
    assert!(DateOnlyTime::new(date, rule.clone(), closed, received, derived).is_err());
    let future = ReferenceVersion::new(
        rule.id(),
        rule.revision(),
        rule.source(),
        UtcInstant::from_unix_millis(derived.unix_millis() + 1)?,
        *rule.checksum(),
        *rule.value(),
    );
    assert!(DateOnlyTime::new(date, future, calendar.clone(), received, derived).is_err());
    let wrong = DateOnlyBucket::new(
        date,
        rule.value().declaration().1,
        SemanticReference::new(calendar.id(), ReferenceRevision::new(4)?),
    )?;
    let wrong = ReferenceVersion::new(
        rule.id(),
        rule.revision(),
        rule.source(),
        rule.known_at(),
        *rule.checksum(),
        wrong,
    );
    assert!(DateOnlyTime::new(date, wrong, calendar.clone(), received, derived).is_err());
    let (start, end) = rule.value().declaration().1.bounds();
    let too_wide = EvidenceInterval::new(
        start,
        PreciseInstant::from_unix_parts(start.unix_parts().0 + 27 * 3600, 0)?,
    )?;
    assert!(DateOnlyBucket::new(date, too_wide, rule.value().declaration().2).is_err());
    let SessionHours::Regular(core) = calendar.value().hours() else {
        return Err("regular fixture".into());
    };
    let short = EvidenceInterval::new(
        PreciseInstant::from_unix_parts(core.bounds().0.unix_millis() / 1000 + 1, 0)?,
        end,
    )?;
    let short = ReferenceVersion::new(
        rule.id(),
        rule.revision(),
        rule.source(),
        rule.known_at(),
        *rule.checksum(),
        DateOnlyBucket::new(date, short, rule.value().declaration().2)?,
    );
    assert!(DateOnlyTime::new(date, short, calendar.clone(), received, derived).is_err());
    Ok(())
}

#[test]
fn date_only_wire_decode_is_bounded_canonical_versioned_and_payload_safe() -> Result {
    let original = freeze_date_only_time(&fixture(
        SessionDate::new(2024, 3, 10)?,
        1_710_046_800,
        23,
        false,
    )?)?;
    let value: serde_json::Value = serde_json::from_slice(original.bytes())?;
    assert!(!format!("{original:?} {:?}", original.data()).contains("synthetic-2026a"));
    for (pointer, replacement) in [
        ("/version", serde_json::json!("edgeagent.evidence.daily.v1")),
        ("/source_date", serde_json::json!("2024-03-11")),
        ("/source_date", serde_json::json!("2024-02-30")),
        ("/calendar/kind", serde_json::json!("mapping")),
        ("/rule/calendar/revision", serde_json::json!("4")),
        ("/derived_at/seconds", serde_json::json!(0)),
        ("/bucket/0/nanos", serde_json::json!(1)),
        ("/received_at/nanos", serde_json::json!(1000000000_u32)),
        ("/timezone", serde_json::json!("UTC")),
        (
            "/tzdata_revision",
            serde_json::json!("private payload sentinel"),
        ),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).ok_or("fixture pointer")? = replacement;
        assert!(
            serde_json::from_value::<DateOnlyTimeEvidence>(changed).is_err(),
            "accepted {pointer}"
        );
    }
    for bytes in [
        vec![b' '; MAX_DATE_ONLY_TIME_BYTES + 1],
        format!(" {}", std::str::from_utf8(original.bytes())?).into_bytes(),
        std::str::from_utf8(original.bytes())?
            .replacen('{', "{\"event_at\":null,", 1)
            .into_bytes(),
        std::str::from_utf8(original.bytes())?
            .replacen(
                '{',
                "{\"version\":\"edgeagent.evidence.date-only-time.v1\",",
                1,
            )
            .into_bytes(),
    ] {
        let error = DateOnlyTimeEvidence::decode(&bytes, &DailyEvidenceArtifact::hash(&bytes))
            .err()
            .ok_or("accepted invalid bytes")?;
        assert!(!format!("{error} {error:?}").contains("sentinel"));
    }
    let mut missing = value.clone();
    missing.as_object_mut().ok_or("object")?.remove("rule");
    assert!(serde_json::from_value::<DateOnlyTimeEvidence>(missing).is_err());
    assert!(
        DateOnlyTimeEvidence::decode(original.bytes(), &DailyEvidenceArtifact::hash(b"wrong"))
            .is_err()
    );
    assert!(serde_json::from_slice::<DailyEvidenceArtifact>(original.bytes()).is_err());
    Ok(())
}
