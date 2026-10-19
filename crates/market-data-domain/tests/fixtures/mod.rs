//! Invented, redistribution-safe reference observations. These sparse dates and
//! hours exercise domain behavior, not authoritative exchange/tzdata coverage.
use edgeagent_market_data_domain::*;

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub fn date(month: u32, day: u32) -> TestResult<SessionDate> {
    Ok(SessionDate::new(2026, month, day)?)
}

pub fn utc(month: u32, day: u32, hour: u32, minute: u32) -> TestResult<UtcInstant> {
    let naive = chrono::NaiveDate::from_ymd_opt(2026, month, day)
        .and_then(|date| date.and_hms_opt(hour, minute, 0))
        .ok_or("invalid synthetic fixture instant")?;
    Ok(UtcInstant::from_unix_millis(
        naive.and_utc().timestamp_millis(),
    )?)
}

pub fn version<T>(
    id: u128,
    revision: u64,
    known: UtcInstant,
    value: T,
) -> TestResult<ReferenceVersion<T>> {
    Ok(ReferenceVersion::new(
        ReferenceId::new(id)?,
        ReferenceRevision::new(revision)?,
        SourceId::new(7)?,
        known,
        [91; 32],
        value,
    ))
}

pub fn mapping(
    instrument: u128,
    start: SessionDate,
    end: SessionDate,
) -> TestResult<InstrumentMapping> {
    Ok(InstrumentMapping::new(
        InstrumentId::new(instrument)?,
        ListingId::new(instrument)?,
        Symbol::new("INVENTED")?,
        Venue::Xnys,
        DateRange::new(start, end)?,
    ))
}

pub fn day(month: u32, day: u32, hours: SessionHours) -> TestResult<CalendarDay> {
    Ok(CalendarDay::new(
        Venue::Xnys,
        date(month, day)?,
        hours,
        "synthetic-2026a",
    )?)
}

pub fn window(
    month: u32,
    day: u32,
    open: u32,
    close: u32,
    close_minute: u32,
) -> TestResult<SessionWindow> {
    Ok(SessionWindow::new(
        utc(month, day, open, 30)?,
        utc(month, day, close, close_minute)?,
    )?)
}
