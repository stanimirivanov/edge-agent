//! Reaccept exact retained native evidence; replay never samples replacement receipt times.
use super::*;
use crate::IexValue;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::{
    AuthorityCatalogs, Cancellation, SelectionPlan, StreamOwnerInputs, StreamingSession,
    verify_stream_capture_record,
};
/// Explicit current authority/reference context for a bounded retained native replay.
pub struct IexReplayContext<'a, 's> {
    /// Complete sealed original session.
    pub session: &'a StreamingSession,
    /// Current exact frozen selection witnesses.
    pub selection: &'a SelectionPlan<'s>,
    /// Trusted current owner catalogs.
    pub catalogs: AuthorityCatalogs<'a>,
    /// Current exact mappings/calendar versions.
    pub references: IexAcceptanceReferences<'a>,
    /// Current evaluation instant, never a replacement market/receipt time.
    pub at: UtcInstant,
    /// Current cancellation signal.
    pub cancellation: &'a Cancellation,
}
impl std::fmt::Debug for IexReplayContext<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IexReplayContext(..)")
    }
}
fn instant(t: EvidenceTime) -> Result<PreciseInstant> {
    PreciseInstant::from_unix_parts(t.seconds, t.nanos).map_err(|_| IexAcceptanceError::Observation)
}
fn price(d: &EvidenceDecimal) -> Result<UsdPrice> {
    UsdPrice::new(
        d.coefficient
            .parse()
            .map_err(|_| IexAcceptanceError::Observation)?,
        u32::from(d.scale),
    )
    .map_err(|_| IexAcceptanceError::Observation)
}
impl IexCanonicalProfile {
    /// Rebuild accepted observations only from exact receipt-bound canonical/native pairs.
    /// Full current profiles/references and original coordinates/receipts must still match.
    /// # Errors
    /// Rejects erased/foreign inputs, changed interpretation, malformed projection or stale authority.
    pub fn replay_companion(
        &self,
        context: IexReplayContext<'_, '_>,
        segment: &StreamSegment,
        native: &StreamCompanion,
    ) -> Result<Vec<IexAcceptedObservation>> {
        let bad = || IexAcceptanceError::Binding;
        native.bind(segment).map_err(|_| bad())?;
        let inputs = StreamOwnerInputs::decode(&native.data().owner_inputs).map_err(|_| bad())?;
        if inputs.profile() != self.bytes()
            || inputs.session_digest() != DailyEvidenceArtifact::hash(context.session.bytes())
        {
            return Err(bad());
        }
        let routes = context
            .selection
            .streaming_routes(context.session)
            .map_err(|_| bad())?;
        let mut values = Vec::with_capacity(segment.data().records.len());
        for (r, n) in segment.data().records.iter().zip(&native.data().records) {
            let mapping = routes
                .iter()
                .find(|route| {
                    format!("{:032x}", route.mapping().value().identities().0.get())
                        == segment.data().scope.instrument.as_str()
                })
                .ok_or_else(bad)?
                .mapping()
                .clone();
            let value = match (&r.value, &n.value) {
                (
                    Some(StreamValue::Trade { price: p, shares }),
                    StreamNativeValue::IexTrade {
                        id,
                        exchange,
                        tape,
                        conditions,
                    },
                ) => IexValue::Trade {
                    id: id.as_str().parse().map_err(|_| bad())?,
                    value: ShareTrade::new(
                        price(p)?,
                        ShareVolume::new(
                            shares.coefficient.parse().map_err(|_| bad())?,
                            u32::from(shares.scale),
                        )
                        .map_err(|_| bad())?,
                    )
                    .map_err(|_| bad())?,
                    exchange: exchange.clone(),
                    tape: tape.clone(),
                    conditions: conditions.clone(),
                },
                (
                    Some(StreamValue::Bar {
                        coverage: StreamBarCoverage::Intraday,
                        bucket,
                        ohlc,
                        volume,
                    }),
                    StreamNativeValue::IexMinuteBar { trades, vwap },
                ) => IexValue::MinuteBar {
                    value: RawBar::new(
                        EvidenceInterval::new(instant(bucket[0])?, instant(bucket[1])?)
                            .map_err(|_| bad())?,
                        BarCoverage::Intraday,
                        [
                            price(&ohlc[0])?,
                            price(&ohlc[1])?,
                            price(&ohlc[2])?,
                            price(&ohlc[3])?,
                        ],
                        ShareVolume::new(
                            volume.coefficient.parse().map_err(|_| bad())?,
                            u32::from(volume.scale),
                        )
                        .map_err(|_| bad())?,
                    )
                    .map_err(|_| bad())?,
                    trades: trades.as_str().parse().map_err(|_| bad())?,
                    vwap: price(vwap)?,
                },
                _ => return Err(bad()),
            };
            if !r.received_at.nanos.is_multiple_of(1_000_000) {
                return Err(bad());
            }
            let input = IexObservation {
                mapping,
                contract: context.session.data().contract,
                event_at: instant(r.event_at)?,
                received_at: UtcInstant::from_unix_millis(
                    r.received_at.seconds * 1000 + i64::from(r.received_at.nanos / 1_000_000),
                )
                .map_err(|_| bad())?,
                frames: [
                    n.frames[0].as_str().parse().map_err(|_| bad())?,
                    n.frames[1].as_str().parse().map_err(|_| bad())?,
                ],
                item: n.item,
                value,
            };
            let accepted = self.accept(
                context.session,
                context.selection,
                context.catalogs,
                IexAcceptanceReferences {
                    mappings: context.references.mappings,
                    calendars: context.references.calendars,
                },
                context.at,
                context.cancellation,
                input,
            )?;
            verify_stream_capture_record(r, accepted.canonical()).map_err(|_| bad())?;
            values.push(accepted);
        }
        let refs: Vec<_> = values.iter().collect();
        if self
            .companion(segment, &inputs, &refs, native.data().limits)?
            .bytes()
            != native.bytes()
        {
            return Err(bad());
        }
        Ok(values)
    }
}
