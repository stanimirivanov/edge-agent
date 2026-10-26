use super::*;
type Result = std::result::Result<(), EvidenceContractError>;
fn number(n: &EvidenceRevision) -> std::result::Result<u64, EvidenceContractError> {
    n.as_str().parse().map_err(|_| EvidenceContractError)
}
pub(super) fn segment(d: &StreamSegmentData) -> Result {
    let error = EvidenceContractError;
    if d.limits.bytes == 0
        || d.limits.bytes as usize > MAX_STREAM_SEGMENT_BYTES
        || d.limits.records == 0
        || d.limits.records as usize > MAX_STREAM_SEGMENT_RECORDS
        || d.records.len() > d.limits.records as usize
        || number(&d.frames[0])? > number(&d.frames[1])?
        || !d.captured_at.valid()
        || !d.delete_by.valid()
        || !d.captured_at.nanos.is_multiple_of(1_000_000)
        || !d.delete_by.nanos.is_multiple_of(1_000_000)
        || d.delete_by <= d.captured_at
    {
        return Err(error);
    }
    let mut last = None;
    let mut versions = std::collections::BTreeMap::<EvidenceVersion, &StreamRecord>::new();
    for r in &d.records {
        let position = (number(&r.frame)?, r.item);
        let revision = number(&r.observation.revision)?;
        if position.0 < number(&d.frames[0])?
            || position.0 > number(&d.frames[1])?
            || last.is_some_and(|p| p >= position)
            || r.conditions.len() > 16
            || r.conditions.windows(2).any(|w| matches!(w,[a,b] if a >= b))
            || (d.delivery == StreamDelivery::Sequenced && r.sequence.is_none())
            || match &r.predecessor {
                None => revision != 1,
                Some(p) => number(p)? >= revision,
            }
            || (r.value.is_none() && r.predecessor.is_none())
        {
            return Err(error);
        }
        last = Some(position);
        if let Some(old) = versions.insert(r.observation.clone(), r) {
            // A real duplicate arrival has another local position/receipt. Preserve it,
            // but do not permit conflicting source facts under one observation revision.
            let mut same = old.clone();
            same.frame.clone_from(&r.frame);
            same.item = r.item;
            same.received_at = r.received_at;
            if same != *r {
                return Err(error);
            }
        }
        let mut at = r.event_at;
        if !at.valid() {
            return Err(error);
        }
        for t in [r.observed_at, r.published_at, Some(r.received_at)]
            .into_iter()
            .flatten()
        {
            if !t.valid() || t < at {
                return Err(error);
            }
            at = t;
        }
        if r.received_at > d.captured_at {
            return Err(error);
        }
        match &r.value {
            None => (),
            Some(StreamValue::Trade { price, shares }) => {
                price.scaled(8, true)?;
                shares.scaled(6, true)?;
            }
            Some(StreamValue::Index { points }) => {
                points.scaled(8, true)?;
            }
            Some(StreamValue::Bar {
                bucket: [a, b],
                ohlc: [o, h, l, c],
                volume,
                ..
            }) => {
                if !a.valid()
                    || !b.valid()
                    || a >= b
                    || r.event_at < *a
                    || r.event_at > *b
                    || r.received_at < *b
                {
                    return Err(error);
                }
                let [o, h, l, c] = [
                    o.scaled(8, true)?,
                    h.scaled(8, true)?,
                    l.scaled(8, true)?,
                    c.scaled(8, true)?,
                ];
                if l > h || o < l || o > h || c < l || c > h {
                    return Err(error);
                }
                volume.scaled(6, false)?;
            }
            Some(StreamValue::OptionQuote { bid, ask, .. })
                if bid.points.scaled(8, false)? > ask.points.scaled(8, false)?
                    || !bid.at.valid()
                    || !ask.at.valid()
                    || bid.at > r.event_at
                    || ask.at > r.event_at =>
            {
                return Err(error);
            }
            Some(StreamValue::OptionQuote { .. }) => (),
        }
    }
    Ok(())
}
