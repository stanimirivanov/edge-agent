//! Project only sealed accepted metadata; raw frames and credentials cannot enter this artifact.
use super::*;
use crate::IexValue;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::{StreamOwnerInputs, verify_stream_capture_record};
impl IexCanonicalProfile {
    /// Bind complete owner inputs and original native values to an exact canonical segment.
    /// Explicit limits and the shared deadline are mandatory. Independent installation is
    /// authenticated again by the publication ledger, not by this deterministic projection.
    /// # Errors
    /// Rejects changed profile, canonical values, source, positions, record count or bounded metadata.
    pub fn companion(
        &self,
        segment: &StreamSegment,
        inputs: &StreamOwnerInputs,
        accepted: &[&IexAcceptedObservation],
        limits: StreamSegmentLimits,
    ) -> Result<StreamCompanion> {
        let reject = || IexAcceptanceError::Binding;
        if inputs.profile() != self.bytes()
            || inputs.claims() != self.namespace_claims()?
            || accepted.len() != segment.data().records.len()
            || accepted.len() > MAX_STREAM_SEGMENT_RECORDS
        {
            return Err(reject());
        }
        let records = accepted
            .iter()
            .zip(&segment.data().records)
            .map(|(a, r)| {
                let n = a.native();
                let (instrument, provider, _) = a.canonical().context().origin();
                if a.profile() != self.digest()
                    || n.frames[1].to_string() != r.frame.as_str()
                    || n.item != r.item
                    || format!("{:032x}", instrument.get())
                        != segment.data().scope.instrument.as_str()
                    || format!("{:032x}", provider.source().get())
                        != segment.data().scope.source.as_str()
                {
                    return Err(reject());
                }
                verify_stream_capture_record(r, a.canonical()).map_err(|_| reject())?;
                let value = match &n.value {
                    IexValue::Trade {
                        id,
                        exchange,
                        tape,
                        conditions,
                        ..
                    } => StreamNativeValue::IexTrade {
                        id: EvidenceRevision::new(&id.to_string()).map_err(|_| reject())?,
                        exchange: exchange.clone(),
                        tape: tape.clone(),
                        conditions: conditions.clone(),
                    },
                    IexValue::MinuteBar { trades, vwap, .. } => {
                        let (coefficient, scale) = vwap.decimal_parts();
                        StreamNativeValue::IexMinuteBar {
                            trades: EvidenceSequence::new(&trades.to_string())
                                .map_err(|_| reject())?,
                            vwap: EvidenceDecimal {
                                coefficient: coefficient.to_string(),
                                scale: u8::try_from(scale).map_err(|_| reject())?,
                            },
                        }
                    }
                };
                Ok(StreamNativeRecord {
                    frames: [
                        EvidenceRevision::new(&n.frames[0].to_string()).map_err(|_| reject())?,
                        r.frame.clone(),
                    ],
                    item: n.item,
                    value,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let companion = StreamCompanion::new(StreamCompanionData {
            version: StreamCompanionFormat::V1,
            limits,
            segment: segment.object(),
            owner_inputs: inputs.bytes().to_vec(),
            records,
        })
        .map_err(|_| reject())?;
        companion.bind(segment).map_err(|_| reject())?;
        Ok(companion)
    }
}
