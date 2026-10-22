use super::{DailyQualityError as Error, DailySeriesSpec};
use crate::{
    CanonicalObservation, DateRange, ObservationValue, ProviderCapability, RawBar, SessionDate,
};
use chrono::{Datelike, NaiveDate};
use std::collections::BTreeMap;

pub(super) fn dates(range: DateRange) -> Result<Vec<SessionDate>, Error> {
    let (start, end) = range.bounds();
    let as_date = |d: SessionDate| {
        let (y, m, d) = d.ymd();
        NaiveDate::from_ymd_opt(y, m, d).ok_or(Error::InvalidScope)
    };
    let (mut date, end) = (as_date(start)?, as_date(end)?);
    if (end - date).num_days() > 366 {
        return Err(Error::InvalidScope);
    }
    let mut dates = Vec::new();
    while date < end {
        dates.push(
            SessionDate::new(date.year(), date.month(), date.day())
                .map_err(|_| Error::InvalidScope)?,
        );
        date = date.succ_opt().ok_or(Error::InvalidScope)?;
    }
    Ok(dates)
}

pub(super) fn select<'a>(
    spec: &DailySeriesSpec<'_>,
    provider: ProviderCapability,
    observations: &'a [CanonicalObservation],
) -> Result<BTreeMap<SessionDate, &'a CanonicalObservation>, Error> {
    if observations.len() > 4096 {
        return Err(Error::InvalidScope);
    }
    let cutoff = crate::PreciseInstant::from_millis(spec.cutoff);
    let mut versions = BTreeMap::new();
    for observation in observations
        .iter()
        .filter(|o| o.times().received_at() <= cutoff)
    {
        let (instrument, origin, _) = observation.context().origin();
        if instrument != spec.instrument || origin != provider {
            return Err(Error::InvalidScope);
        }
        let (id, revision, _) = observation.version().lineage();
        if versions
            .insert((id, revision), observation)
            .is_some_and(|old| old != observation)
        {
            return Err(Error::Revision);
        }
    }
    let mut selected: BTreeMap<_, &CanonicalObservation> = BTreeMap::new();
    for ((id, _), observation) in versions {
        let (_, _, replaces) = observation.version().lineage();
        let previous = selected.get(&id).copied();
        if replaces != previous.map(|p| p.version().lineage().1)
            || previous.is_some_and(|p| p.times().received_at() > observation.times().received_at())
        {
            return Err(Error::Revision);
        }
        selected.insert(id, observation);
    }
    let mut days = BTreeMap::new();
    for observation in selected.into_values() {
        // Select the latest known revision BEFORE filtering effective dates/cancellation.
        let date = observation.context().session_conditions().0;
        if !spec.range.contains(date) || observation.value().is_none() {
            continue;
        }
        if days.insert(date, observation).is_some() {
            return Err(Error::Completeness);
        }
    }
    Ok(days)
}

pub(super) fn bar(
    observation: &CanonicalObservation,
    interval: crate::EvidenceInterval,
) -> Result<RawBar, Error> {
    let Some(ObservationValue::Bar(bar)) = observation.value() else {
        return Err(Error::Completeness);
    };
    if bar.bucket() != (interval, crate::BarCoverage::ProviderDay) {
        return Err(Error::Bucket);
    }
    Ok(bar)
}
