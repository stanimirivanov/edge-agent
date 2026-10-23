use crate::OfflineError;
use edgeagent_market_data_application::{AcquisitionRequest, RawRetention};
use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::ArtifactDigest;

pub(super) fn bind_request(
    r: &AcquisitionRequest,
    configuration: ArtifactDigest,
) -> ArtifactDigest {
    let mut bytes = b"EA-SYNTHETIC-REQUEST1\0".to_vec();
    bytes.extend(configuration.bytes());
    for id in [
        r.id.get(),
        r.scope.principal.get(),
        r.scope.account.get(),
        r.scope.source.get(),
        r.scope.instrument.get(),
        r.scope.endpoint.get(),
        r.scope.audience.get(),
        r.scope.storage.get(),
    ] {
        bytes.extend(id.to_be_bytes());
    }
    for day in [r.range.bounds().0, r.range.bounds().1] {
        let (year, month, day) = day.ymd();
        bytes.extend(year.to_be_bytes());
        bytes.extend(month.to_be_bytes());
        bytes.extend(day.to_be_bytes());
    }
    bytes.extend(r.admitted_at.unix_millis().to_be_bytes());
    bytes.extend(r.deadline.unix_millis().to_be_bytes());
    bytes.push(u8::from(r.raw == RawRetention::Retain));
    for limit in [
        r.budget.connect_ms,
        r.budget.request_ms,
        r.budget.calls,
        r.budget.pages,
        r.budget.response_bytes,
        r.budget.command_bytes,
    ] {
        bytes.extend(limit.get().to_be_bytes());
    }
    ArtifactDigest::of(&bytes)
}

/// Canonical invented daily record v1: fixed-width big-endian fields, bounded condition
/// count and explicit optional-value tags. This is fixture provenance, not a public event.
/// # Errors
/// Rejects non-provider-day bars, cancellations, sequenced/historical capabilities or
/// unsupported discovery scope/precision. Never use this encoder for licensed raw capture.
pub fn synthetic_daily_bytes(observation: &CanonicalObservation) -> Result<Vec<u8>, OfflineError> {
    let (instrument, provider, transformation) = observation.context().origin();
    let (contract, delivery, knowledge) = provider.declaration();
    let (semantics, precision) = contract.rules();
    let venue = match contract.shape() {
        (EvidenceKind::RawBar(BarCoverage::ProviderDay), MarketScope::Venue(Venue::Xnys)) => 1,
        (EvidenceKind::RawBar(BarCoverage::ProviderDay), MarketScope::Venue(Venue::Xnas)) => 2,
        (EvidenceKind::RawBar(BarCoverage::ProviderDay), MarketScope::Venue(Venue::Arcx)) => 3,
        _ => return Err(OfflineError::Fixture),
    };
    if precision != TimestampPrecision::Milliseconds
        || delivery != DeliveryCapability::Snapshots
        || knowledge != KnowledgeCapability::CaptureOnly
    {
        return Err(OfflineError::Fixture);
    }
    let Some(ObservationValue::Bar(bar)) = observation.value() else {
        return Err(OfflineError::Fixture);
    };
    let (id, revision, replaces) = observation.version().lineage();
    let (session, conditions) = observation.context().session_conditions();
    let (semantic_id, semantic_revision) = semantics.version();
    let mut bytes = b"EA-SYNTHETIC-DAILY1\0".to_vec();
    bytes.push(venue);
    bytes.extend(instrument.get().to_be_bytes());
    bytes.extend(provider.source().get().to_be_bytes());
    bytes.extend(transformation.get().to_be_bytes());
    bytes.extend(semantic_id.get().to_be_bytes());
    bytes.extend(semantic_revision.get().to_be_bytes());
    bytes.extend(id.get().to_be_bytes());
    bytes.extend(revision.get().to_be_bytes());
    optional(&mut bytes, replaces.map(|r| r.get()));
    optional(&mut bytes, observation.sequence());
    let (year, month, day) = session.ymd();
    bytes.extend(year.to_be_bytes());
    bytes.extend(month.to_be_bytes());
    bytes.extend(day.to_be_bytes());
    bytes.extend((conditions.len() as u32).to_be_bytes());
    for code in conditions {
        bytes.extend(code.to_be_bytes());
    }
    let times = observation.times();
    instant(&mut bytes, times.event_at());
    for value in [times.provider_times().0, times.provider_times().1] {
        bytes.push(u8::from(value.is_some()));
        if let Some(value) = value {
            instant(&mut bytes, value);
        }
    }
    instant(&mut bytes, times.received_at());
    instant(&mut bytes, bar.bucket().0.bounds().0);
    instant(&mut bytes, bar.bucket().0.bounds().1);
    let (prices, volume) = bar.values();
    for (coefficient, scale) in prices
        .map(UsdPrice::decimal_parts)
        .into_iter()
        .chain([volume.decimal_parts()])
    {
        bytes.extend(coefficient.to_be_bytes());
        bytes.extend(scale.to_be_bytes());
    }
    Ok(bytes)
}
fn optional(bytes: &mut Vec<u8>, value: Option<u64>) {
    bytes.push(u8::from(value.is_some()));
    if let Some(value) = value {
        bytes.extend(value.to_be_bytes());
    }
}
fn instant(bytes: &mut Vec<u8>, at: PreciseInstant) {
    let (seconds, nanos) = at.unix_parts();
    bytes.extend(seconds.to_be_bytes());
    bytes.extend(nanos.to_be_bytes());
}
