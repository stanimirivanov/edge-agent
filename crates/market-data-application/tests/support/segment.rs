//! Shared invented segment through the real session and domain constructors.
#[allow(dead_code)]
#[path = "streaming.rs"]
pub mod session_fixture;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
pub use session_fixture::{Fixture, Result, at};
pub const AT: i64 = 1_800_000_000_000;
pub fn observation(f: &Fixture, id: u128, sequence: u64) -> Result<CanonicalObservation> {
    Ok(CanonicalObservation::new(
        ObservationContext::new(
            f.scopes.first().ok_or("scope")?.instrument,
            ProviderCapability::new(
                SourceId::new(1)?,
                f.contract,
                DeliveryCapability::Sequenced,
                KnowledgeCapability::CaptureOnly,
            ),
            TransformationRevision::new(1)?,
            SessionDate::new(2027, 1, 15)?,
            &[1, 3],
        )?,
        ObservationVersion::new(ObservationId::new(id)?, ObservationRevision::new(1)?, None)?,
        EvidenceTimes::new(
            PreciseInstant::from_millis(at(AT)?),
            None,
            None,
            PreciseInstant::from_millis(at(AT + 1)?),
        )?,
        Some(sequence),
        Some(ObservationValue::Trade(ShareTrade::new(
            UsdPrice::new(12345, 2)?,
            ShareVolume::new(7, 0)?,
        )?)),
    )?)
}
pub fn connection(session: &StreamingSession) -> Result<StreamingConnection> {
    Ok(StreamingConnection::from_committed(
        session,
        7,
        StreamingUsage {
            starts: 1,
            frames: 0,
            bytes: 0,
            last_frame_bytes: None,
            stopped: false,
        },
    )?)
}
pub fn input<'a>(
    f: &Fixture,
    records: &'a [CapturedStreamObservation<'a>],
) -> Result<StreamSegmentInput<'a>> {
    Ok(StreamSegmentInput {
        instrument: f.scopes.first().ok_or("scope")?.instrument,
        epoch: FeedEpoch::new(3)?,
        frames: [1, 3],
        previous: None,
        limits: StreamSegmentLimits {
            bytes: 65536,
            records: 16,
        },
        delete_by: at(AT + 1000)?,
        records,
    })
}
pub fn segment() -> Result<StreamSegment> {
    let f = Fixture::new(AT, 1)?;
    let s = f.session(1)?;
    let o = observation(&f, 10, 0)?;
    let records = [CapturedStreamObservation {
        frame: 2,
        item: 4,
        observation: &o,
    }];
    Ok(freeze_stream_segment(
        &s,
        &connection(&s)?,
        f.catalogs(),
        at(AT + 100)?,
        &Cancellation::default(),
        input(&f, &records)?,
    )?)
}
