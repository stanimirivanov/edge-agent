//! Rehydrate retained canonical inputs and rerun the original whole-series quality path.
//! Recovery cannot turn an arbitrary artifact into a publication proposal.
use super::*;
pub(super) struct Checkpoint<'a, S> {
    pub ledger: &'a Ledger,
    pub storage: &'a S,
}
impl<S: EvidenceStorage> EvidenceStorage for Checkpoint<'_, S> {
    async fn put(
        &self,
        a: &DailyEvidenceArtifact,
        access: &impl EvidenceAccess,
    ) -> std::result::Result<EvidenceObject, EvidenceStorageError> {
        self.ledger
            .stage_precise_preparation(a)
            .await
            .map_err(|e| EvidenceStorageError::with_source(EvidenceStorageErrorKind::Denied, e))?;
        self.storage.put(a, access).await
    }
    async fn get(
        &self,
        o: &EvidenceObject,
        access: &impl EvidenceAccess,
    ) -> std::result::Result<DailyEvidenceArtifact, EvidenceStorageError> {
        self.storage.get(o, access).await
    }
}
fn instant(t: EvidenceTime) -> Result<PreciseInstant> {
    PreciseInstant::from_unix_parts(t.seconds, t.nanos).map_err(invalid)
}
pub(super) fn captured(a: &DailyEvidenceArtifact) -> Result<UtcInstant> {
    let t = a.data().captured_at;
    if !t.nanos.is_multiple_of(1_000_000) {
        return Err(invalid(()));
    }
    UtcInstant::from_unix_millis(
        i64::try_from(i128::from(t.seconds) * 1000 + i128::from(t.nanos / 1_000_000))
            .map_err(invalid)?,
    )
    .map_err(invalid)
}
pub(super) fn batch(
    a: &DailyEvidenceArtifact,
    r: &AdmissionRecord,
    p: ProviderCapability,
) -> Result<ProviderBatch> {
    let data = a.data();
    let observations = data
        .bars
        .iter()
        .map(|b| -> Result<_> {
            let parts = b
                .date
                .as_str()
                .split('-')
                .map(str::parse::<u32>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(invalid)?;
            let [y, m, d] = parts.as_slice() else {
                return Err(invalid(()));
            };
            let day =
                SessionDate::new(i32::try_from(*y).map_err(invalid)?, *m, *d).map_err(invalid)?;
            let ident = u128::from_str_radix(b.observation.id.as_str(), 16).map_err(invalid)?;
            let version = ObservationVersion::new(
                ObservationId::new(ident).map_err(invalid)?,
                ObservationRevision::new(b.observation.revision.as_str().parse().map_err(invalid)?)
                    .map_err(invalid)?,
                b.predecessor
                    .as_ref()
                    .map(|v| {
                        ObservationRevision::new(v.as_str().parse().map_err(invalid)?)
                            .map_err(invalid)
                    })
                    .transpose()?,
            )
            .map_err(invalid)?;
            let price = |v: &EvidenceDecimal| {
                UsdPrice::new(v.coefficient.parse().map_err(invalid)?, u32::from(v.scale))
                    .map_err(invalid)
            };
            let ohlc = b
                .ohlc
                .iter()
                .map(price)
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .map_err(invalid)?;
            let raw = RawBar::new(
                EvidenceInterval::new(instant(b.bucket[0])?, instant(b.bucket[1])?)
                    .map_err(invalid)?,
                BarCoverage::ProviderDay,
                ohlc,
                ShareVolume::new(
                    b.volume.coefficient.parse().map_err(invalid)?,
                    u32::from(b.volume.scale),
                )
                .map_err(invalid)?,
            )
            .map_err(invalid)?;
            CanonicalObservation::new(
                ObservationContext::new(
                    r.request().scope.instrument,
                    p,
                    TransformationRevision::new(
                        b.transformation.as_str().parse().map_err(invalid)?,
                    )
                    .map_err(invalid)?,
                    day,
                    &b.conditions,
                )
                .map_err(invalid)?,
                version,
                EvidenceTimes::new(
                    instant(b.event_at)?,
                    b.observed_at.map(instant).transpose()?,
                    b.published_at.map(instant).transpose()?,
                    instant(b.received_at)?,
                )
                .map_err(invalid)?,
                b.sequence
                    .as_ref()
                    .map(|s| s.as_str().parse().map_err(invalid))
                    .transpose()?,
                Some(ObservationValue::Bar(raw)),
            )
            .map_err(invalid)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ProviderBatch {
        request: r.request().id,
        provider: p,
        observations,
    })
}
