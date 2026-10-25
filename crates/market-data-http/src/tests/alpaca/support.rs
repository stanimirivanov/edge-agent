use super::*;
pub fn schedule(f: &Fixture) -> TestResult<PreciseDailySchedule> {
    shifted_schedule(f, 0)
}
// Invented UTC calendars for boundary tests, not a licensed exchange schedule.
pub fn shifted_schedule(f: &Fixture, shift: i64) -> TestResult<PreciseDailySchedule> {
    let scope = RoutingScope {
        source: f.request.scope.source,
        feed: FeedId::new(1)?,
        epoch: FeedEpoch::new(1)?,
    };
    let effective = EffectiveInterval::new(at(0)?, at(NOW + 86400000)?)?;
    let mapping = ReferenceVersion::new(
        ReferenceId::new(4)?,
        ReferenceRevision::new(1)?,
        scope.source,
        at(0)?,
        [4; 32],
        InstrumentMapping::new(
            f.request.scope.instrument,
            ListingId::new(1)?,
            Symbol::new("INVENTED")?,
            Venue::Xnys,
            f.request.range,
        ),
    );
    let directories = ReferenceHistory::new(vec![ReferenceVersion::new(
        ReferenceId::new(5)?,
        ReferenceRevision::new(1)?,
        scope.source,
        at(0)?,
        [5; 32],
        RoutingDirectory::new(
            scope,
            effective,
            vec![RouteBinding::new(
                RoutingKey::symbol(&Symbol::new("INVENTED")?),
                mapping,
            )?],
        )?,
    )])?;
    let selections = ReferenceHistory::new(vec![ReferenceVersion::new(
        ReferenceId::new(6)?,
        ReferenceRevision::new(1)?,
        scope.source,
        at(0)?,
        [6; 32],
        InstrumentSelection::new(effective, &[f.request.scope.instrument])?,
    )])?;
    let plan = SelectionPlan::resolve(
        SelectionContext {
            scope,
            effective_at: f.request.admitted_at,
            cutoff: f.request.admitted_at,
            session: f.request.range.bounds().0,
        },
        (ReferenceId::new(6)?, ReferenceId::new(5)?),
        &selections,
        &directories,
    )?;
    let selected = plan.prepare(&f.request)?;
    let mut calendars = Vec::new();
    let mut slots = Vec::new();
    for (offset, day) in [(0, 30), (1, 31)] {
        let start = -172800000 + offset * 86400000 + shift;
        let date = match shift {
            0 => SessionDate::new(1969, 12, day)?,
            NOW => SessionDate::new(1970, 1, day - 29)?,
            _ => return Err("unsupported invented calendar shift".into()),
        };
        calendars.push(ReferenceVersion::new(
            ReferenceId::new(day.into())?,
            ReferenceRevision::new(1)?,
            scope.source,
            at(0)?,
            [2; 32],
            CalendarDay::new(
                Venue::Xnys,
                date,
                SessionHours::Regular(SessionWindow::new(
                    at(start + 3600000)?,
                    at(start + 7200000)?,
                )?),
                "synthetic-2026a",
            )?,
        ));
        slots.push(DailyObservationSlot {
            date,
            bucket: EvidenceInterval::new(
                PreciseInstant::from_unix_parts(start / 1000, 7)?,
                PreciseInstant::from_millis(at(start + 86400000)?),
            )?,
            version: ObservationVersion::new(
                ObservationId::new(u128::from(day))?,
                ObservationRevision::new(1)?,
                None,
            )?,
        });
    }
    assert!(
        PreciseDailySchedule::new(&selected, &[], &slots, TransformationRevision::new(7)?).is_err()
    );
    assert!(
        PreciseDailySchedule::new(
            &selected,
            &calendars,
            &slots[..1],
            TransformationRevision::new(7)?
        )
        .is_err()
    );
    let result = PreciseDailySchedule::new(
        &selected,
        &calendars,
        &slots,
        TransformationRevision::new(7)?,
    )?;
    let changed = PreciseDailySchedule::new(
        &selected,
        &calendars,
        &slots,
        TransformationRevision::new(8)?,
    )?;
    assert_ne!(
        result.configuration_digest(),
        changed.configuration_digest()
    );
    Ok(result)
}
pub fn ledger(f: &Fixture, schedule: &PreciseDailySchedule, opening: u32) -> TestResult<Ledger> {
    let mut ledger = Ledger::with_opening_debit(f, opening)?;
    ledger.record = ledger
        .record
        .with_configuration(schedule.configuration_digest())?;
    Ok(ledger)
}
pub fn config(schedule: PreciseDailySchedule) -> TestResult<AlpacaDailyConfig<'static>> {
    Ok(AlpacaDailyConfig {
        schedule,
        key_id: "invented-local-id",
        secret: "invented-local-token",
        cancellation_poll_ms: nz(5)?,
    })
}
pub fn row(index: usize) -> String {
    let date = if index == 0 {
        "1969-12-30"
    } else {
        "1969-12-31"
    };
    format!(
        r#"{{"t":"{date}T00:00:00.000000007Z","o":9007199254740993.12345678,"h":9007199254740993.12345679,"l":9007199254740993.12345677,"c":9007199254740993.12345678,"v":0}}"#
    )
}
pub fn page(rows: &[String], next: Option<&str>) -> Vec<u8> {
    let next = next.map_or_else(
        || "null".to_owned(),
        |s| serde_json::to_string(s).unwrap_or_default(),
    );
    format!(
        r#"{{"bars":{{"INVENTED":[{}]}},"next_page_token":{next}}}"#,
        rows.join(",")
    )
    .into_bytes()
}
