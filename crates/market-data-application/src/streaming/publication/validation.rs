//! Revalidate reconstructed private capture; a valid codec or checksum is not authority.
use super::*;
use edgeagent_market_data_domain::{BarCoverage, EvidenceKind, TimestampPrecision};
fn millis(t: EvidenceTime) -> Result<i64> {
    i64::try_from(i128::from(t.seconds) * 1000 + i128::from(t.nanos / 1_000_000))
        .map_err(|_| invalid())
}
impl StreamPublication {
    /// Recheck a reconstructed proposal against complete current owner-retained admission.
    /// This checks data meaning, not durable charged-frame ownership or object persistence.
    /// # Errors
    /// Rejects changed scope, semantics, value class, receipt window, rights or retention.
    pub fn recheck_capture(
        &self,
        session: &StreamingSession,
        catalogs: AuthorityCatalogs<'_>,
        at: UtcInstant,
        cancel: &Cancellation,
    ) -> Result<()> {
        session.recheck(catalogs, at, cancel)?;
        let d = self.segment.data();
        let scope = session
            .data()
            .scopes
            .iter()
            .find(|s| format!("{:032x}", s.instrument.get()) == d.scope.instrument.as_str())
            .ok_or_else(invalid)?;
        let r = catalogs
            .rights
            .at(at)
            .find(|r| r.value().scope == *scope && r.value().contract == session.data().contract)
            .ok_or_else(invalid)?;
        let hex = |v: u128| format!("{v:032x}");
        let semantics = session.data().contract.rules().0.version();
        let delivery = match session.data().delivery {
            DeliveryCapability::Snapshots => StreamDelivery::Snapshots,
            DeliveryCapability::Sequenced => StreamDelivery::Sequenced,
            _ => return Err(invalid()),
        };
        if d.scope.session != session.data().id
            || d.scope.session_sha256 != DailyEvidenceArtifact::hash(session.bytes())
            || d.scope.principal.as_str() != hex(scope.principal.get())
            || d.scope.account.as_str() != hex(scope.account.get())
            || d.scope.source.as_str() != hex(scope.source.get())
            || d.scope.endpoint.as_str() != hex(scope.endpoint.get())
            || d.scope.audience.as_str() != hex(scope.audience.get())
            || d.scope.storage.as_str() != hex(scope.storage.get())
            || d.scope.rights.id.as_str() != hex(r.id().get())
            || d.scope.rights.revision.as_str() != r.revision().get().to_string()
            || d.semantics.id.as_str() != hex(semantics.0.get())
            || d.semantics.revision.as_str() != semantics.1.get().to_string()
            || d.delivery != delivery
            || millis(d.delete_by)? > r.value().delete_by.unix_millis()
        {
            return Err(Error::new(K::Denied));
        }
        let capture = millis(d.captured_at)?;
        let (begin, end) = session.data().window.bounds();
        if capture < begin.unix_millis()
            || capture >= end.unix_millis()
            || capture > at.unix_millis()
        {
            return Err(invalid());
        }
        let mut earliest = capture;
        for record in &d.records {
            let quantum = match session.data().contract.rules().1 {
                TimestampPrecision::Milliseconds => 1_000_000,
                TimestampPrecision::Microseconds => 1_000,
                TimestampPrecision::Nanoseconds => 1,
                _ => return Err(invalid()),
            };
            if [
                Some(record.event_at),
                record.observed_at,
                record.published_at,
            ]
            .into_iter()
            .flatten()
            .any(|t| !t.nanos.is_multiple_of(quantum))
            {
                return Err(invalid());
            }
            let receipt = millis(record.received_at)?;
            if receipt < begin.unix_millis() || receipt > capture {
                return Err(invalid());
            }
            earliest = earliest.min(receipt);
            let matches = matches!(
                (&record.value, session.data().contract.shape().0),
                (None, _)
                    | (Some(StreamValue::Trade { .. }), EvidenceKind::ShareTrade)
                    | (Some(StreamValue::Index { .. }), EvidenceKind::IndexLevel)
                    | (
                        Some(StreamValue::OptionQuote { .. }),
                        EvidenceKind::OptionSnapshot
                    )
                    | (
                        Some(StreamValue::Bar {
                            coverage: StreamBarCoverage::Core,
                            ..
                        }),
                        EvidenceKind::RawBar(BarCoverage::CoreSession),
                    )
                    | (
                        Some(StreamValue::Bar {
                            coverage: StreamBarCoverage::Intraday,
                            ..
                        }),
                        EvidenceKind::RawBar(BarCoverage::Intraday),
                    )
            );
            if !matches {
                return Err(invalid());
            }
        }
        if millis(d.delete_by)? - earliest > i64::from(r.value().canonical_retention_ms.get()) {
            return Err(Error::new(K::Denied));
        }
        Ok(())
    }
}
