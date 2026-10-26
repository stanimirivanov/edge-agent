//! Pure capture freezing. Durable charged-frame ownership is rechecked by PR17B.
use super::*;
use crate::evidence::{date, decimal, id, millis, revision, time, version};
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_domain::*;

/// One selected normalized record; local position is not source sequencing.
#[derive(Debug)]
pub struct CapturedStreamObservation<'a> {
    /// Charged session-local frame ordinal.
    pub frame: u64,
    /// Original zero-based position within a multi-record frame, before selection.
    pub item: u32,
    /// Already structurally checked domain observation, rechecked against the session.
    pub observation: &'a CanonicalObservation,
}
/// Explicit segment proposal; no identity, limit, epoch or retention defaults.
#[derive(Debug)]
pub struct StreamSegmentInput<'a> {
    /// One instrument selected by the complete admitted session.
    pub instrument: InstrumentId,
    /// Explicit source-feed epoch, not inferred from the connection generation.
    pub epoch: FeedEpoch,
    /// Inclusive local capture range; may include only controls/filtered frames.
    pub frames: [u64; 2],
    /// Exact prior segment, if any. This does not verify a durable chain head.
    pub previous: Option<&'a EvidenceDigest>,
    /// Explicit bounded segment capacity.
    pub limits: StreamSegmentLimits,
    /// Exclusive requested deadline; must fit original licensed retention.
    pub delete_by: UtcInstant,
    /// Canonical frame/item order, without sorting or silently dropping input.
    pub records: &'a [CapturedStreamObservation<'a>],
}
/// Recheck sealed authority, map normalized values losslessly and freeze exact bytes.
/// This is capture, not quality acceptance, durable accounting, continuity or publication.
/// Full session/selection/reference records must remain authenticated and retained.
/// # Errors
/// Rejects denied/expired authority, cancellation, scope/contract/range mismatch,
/// retention extension or malformed/oversized input. Performs no I/O.
pub fn freeze_stream_segment(
    session: &StreamingSession,
    connection: &StreamingConnection,
    catalogs: AuthorityCatalogs<'_>,
    now: UtcInstant,
    cancellation: &Cancellation,
    input: StreamSegmentInput<'_>,
) -> Result<StreamSegment> {
    session.recheck(catalogs, now, cancellation)?;
    let convert = || -> std::result::Result<StreamSegment, EvidenceContractError> {
        let d = session.data();
        if !connection.matches(session)
            || input.frames[1] > u64::from(d.limits.frames.get())
            || input.records.len() > MAX_STREAM_SEGMENT_RECORDS
        {
            return Err(EvidenceContractError);
        }
        let scope = d
            .scopes
            .iter()
            .find(|s| s.instrument == input.instrument)
            .ok_or(EvidenceContractError)?;
        // Session recheck already proved exactly one current matching grant and pinned its values.
        let rights = catalogs
            .rights
            .at(now)
            .find(|r| r.value().scope == *scope && r.value().contract == d.contract)
            .ok_or(EvidenceContractError)?;
        let mut retention_start = now.unix_millis();
        let mut records = Vec::with_capacity(input.records.len());
        for captured in input.records {
            let o = captured.observation;
            let (instrument, provider, transformation) = o.context().origin();
            let (contract, delivery, _) = provider.declaration();
            let received = o.times().received_at();
            if instrument != scope.instrument
                || provider.source() != scope.source
                || contract != d.contract
                || delivery != d.delivery
                || received < PreciseInstant::from_millis(d.window.bounds().0)
                || received >= PreciseInstant::from_millis(d.window.bounds().1)
                || received > PreciseInstant::from_millis(now)
            {
                return Err(EvidenceContractError);
            }
            let received_time = time(received)?;
            // Round the licensed deletion ceiling down, never extend it by submillisecond receipt precision.
            let received_ms =
                received_time.seconds * 1000 + i64::from(received_time.nanos / 1_000_000);
            retention_start = retention_start.min(received_ms);
            let (oid, rev, previous) = o.version().lineage();
            let (observed, published) = o.times().provider_times();
            records.push(StreamRecord {
                frame: revision(captured.frame)?,
                item: captured.item,
                observation: version(oid.get(), rev.get())?,
                predecessor: previous.map(|r| revision(r.get())).transpose()?,
                transformation: revision(transformation.get())?,
                sequence: o
                    .sequence()
                    .map(|n| EvidenceSequence::new(&n.to_string()))
                    .transpose()?,
                event_at: time(o.times().event_at())?,
                received_at: time(received)?,
                observed_at: observed.map(time).transpose()?,
                published_at: published.map(time).transpose()?,
                date: date(o.context().session_conditions().0)?,
                conditions: o.context().session_conditions().1.to_vec(),
                value: o.value().map(value).transpose()?,
            });
        }
        if input.delete_by > rights.value().delete_by
            || input.delete_by.unix_millis() - retention_start
                > i64::from(rights.value().canonical_retention_ms.get())
        {
            return Err(EvidenceContractError);
        }
        let semantics = d.contract.rules().0.version();
        StreamSegment::new(StreamSegmentData {
            version: StreamSegmentFormat::V1,
            limits: input.limits,
            scope: StreamSegmentScope {
                session: d.id.clone(),
                session_sha256: DailyEvidenceArtifact::hash(session.bytes()),
                principal: id(scope.principal.get())?,
                account: id(scope.account.get())?,
                source: id(scope.source.get())?,
                instrument: id(scope.instrument.get())?,
                endpoint: id(scope.endpoint.get())?,
                audience: id(scope.audience.get())?,
                storage: id(scope.storage.get())?,
                rights: version(rights.id().get(), rights.revision().get())?,
            },
            semantics: version(semantics.0.get(), semantics.1.get())?,
            delivery: match d.delivery {
                DeliveryCapability::Snapshots => StreamDelivery::Snapshots,
                DeliveryCapability::Sequenced => StreamDelivery::Sequenced,
                _ => return Err(EvidenceContractError),
            },
            epoch: revision(input.epoch.get())?,
            generation: revision(connection.generation())?,
            frames: [revision(input.frames[0])?, revision(input.frames[1])?],
            previous: input.previous.cloned(),
            captured_at: millis(now)?,
            delete_by: millis(input.delete_by)?,
            records,
        })
    };
    convert().map_err(|_| invalid())
}
fn side(s: QuoteSide) -> std::result::Result<StreamQuoteSide, EvidenceContractError> {
    Ok(StreamQuoteSide {
        points: decimal(s.values().0.decimal_parts())?,
        size: s
            .values()
            .1
            .map(|v| EvidenceSequence::new(&v.get().to_string()))
            .transpose()?,
        at: time(s.at())?,
    })
}
fn value(v: ObservationValue) -> std::result::Result<StreamValue, EvidenceContractError> {
    Ok(match v {
        ObservationValue::Trade(t) => StreamValue::Trade {
            price: decimal(t.values().0.decimal_parts())?,
            shares: decimal(t.values().1.decimal_parts())?,
        },
        ObservationValue::IndexLevel(p) => StreamValue::Index {
            points: decimal(p.decimal_parts())?,
        },
        ObservationValue::Bar(b) => {
            let [o, h, l, c] = b.values().0;
            StreamValue::Bar {
                coverage: match b.bucket().1 {
                    BarCoverage::CoreSession => StreamBarCoverage::Core,
                    BarCoverage::Intraday => StreamBarCoverage::Intraday,
                    _ => return Err(EvidenceContractError),
                },
                bucket: [
                    time(b.bucket().0.bounds().0)?,
                    time(b.bucket().0.bounds().1)?,
                ],
                ohlc: [
                    decimal(o.decimal_parts())?,
                    decimal(h.decimal_parts())?,
                    decimal(l.decimal_parts())?,
                    decimal(c.decimal_parts())?,
                ],
                volume: decimal(b.values().1.decimal_parts())?,
            }
        }
        ObservationValue::OptionQuote(q) => StreamValue::OptionQuote {
            specification: version(q.specification().0.get(), q.specification().1.get())?,
            bid: side(q.sides().0)?,
            ask: side(q.sides().1)?,
        },
        _ => return Err(EvidenceContractError),
    })
}
