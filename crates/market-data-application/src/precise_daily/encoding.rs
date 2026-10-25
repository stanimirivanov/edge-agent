use super::*;
// Owner-private versioned fixed-width/length-delimited primitives, never Debug or
// provider JSON. Include values as well as supplied witnesses to expose stale pins.
pub(super) fn binding(
    selected: &SelectedRequest<'_>,
    dates: BTreeMap<SessionDate, &ReferenceVersion<CalendarDay>>,
    slots: &[DailyObservationSlot],
    transformation: TransformationRevision,
) -> Result<Vec<u8>, AcquisitionError> {
    let mut out = b"edgeagent.precise-daily-schedule.v1\0".to_vec();
    macro_rules! n {
        ($v:expr) => {
            out.extend_from_slice(&$v.to_be_bytes())
        };
    }
    macro_rules! date {
        ($v:expr) => {{
            let (y, m, d) = $v.ymd();
            n!(y);
            n!(m);
            n!(d);
        }};
    }
    let selection = selected
        .binding_bytes()
        .map_err(|_| AcquisitionError::Conformance)?;
    n!(selection.len() as u64);
    out.extend_from_slice(&selection);
    n!(transformation.get());
    n!(dates.len() as u64);
    for (label, c) in dates {
        date!(label);
        n!(c.id().get());
        n!(c.revision().get());
        n!(c.source().get());
        n!(c.known_at().unix_millis());
        out.extend_from_slice(c.checksum());
        let tz = c.value().timezone().1.as_bytes();
        n!(tz.len() as u64);
        out.extend_from_slice(tz);
        let (tag, window) = match c.value().hours() {
            SessionHours::Closed => (0, None),
            SessionHours::Regular(w) => (1, Some(w)),
            SessionHours::EarlyClose(w) => (2, Some(w)),
            _ => return Err(AcquisitionError::Conformance),
        };
        out.push(tag);
        if let Some(w) = window {
            let (a, b) = w.bounds();
            n!(a.unix_millis());
            n!(b.unix_millis());
        }
    }
    n!(slots.len() as u64);
    for s in slots {
        date!(s.date);
        let (a, b) = s.bucket.bounds();
        for at in [a, b] {
            let (seconds, nanos) = at.unix_parts();
            n!(seconds);
            n!(nanos);
        }
        let (id, revision, previous) = s.version.lineage();
        n!(id.get());
        n!(revision.get());
        n!(previous.map_or(0, ObservationRevision::get));
    }
    Ok(out)
}
