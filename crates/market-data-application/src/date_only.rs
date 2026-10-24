//! Lossless native-date provenance mapping; no provider call or source timestamp synthesis.
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_domain::*;

/// Seal a checked date-only temporal value for retention in a future observation format.
/// Does not create a daily-v1 artifact, grant rights or infer earlier availability.
/// # Errors
/// Rejects a temporal representation inconsistent with the versioned wire contract.
pub fn freeze_date_only_time(
    value: &DateOnlyTime,
) -> Result<DateOnlyTimeEvidence, EvidenceContractError> {
    let id = |n: u128| EvidenceId::new(&format!("{n:032x}"));
    let version = |id_value: ReferenceId, rev: ReferenceRevision| {
        Ok::<_, EvidenceContractError>(EvidenceVersion {
            id: id(id_value.get())?,
            revision: EvidenceRevision::new(&rev.get().to_string())?,
        })
    };
    let time = |at: PreciseInstant| {
        let (seconds, nanos) = at.unix_parts();
        Ok::<_, EvidenceContractError>(EvidenceTime {
            seconds,
            nanos: u32::try_from(nanos).map_err(|_| EvidenceContractError)?,
        })
    };
    let millis = |at| time(PreciseInstant::from_millis(at));
    let hash = |checksum: &[u8; 32]| {
        EvidenceDigest::new(
            &checksum
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
        )
    };
    let (rule, calendar) = value.provenance();
    let (y, m, d) = value.source_date().ymd();
    let date = EvidenceDate::new(&format!("{y:04}-{m:02}-{d:02}"))?;
    let calendar_version = version(calendar.id(), calendar.revision())?;
    let (begin, end) = rule.value().declaration().1.bounds();
    let (received, derived) = value.local_times();
    DateOnlyTimeEvidence::new(DateOnlyTimeData {
        version: DateOnlyTimeFormat::V1,
        source_date: date.clone(),
        bucket: [time(begin)?, time(end)?],
        received_at: time(received)?,
        derived_at: millis(derived)?,
        rule: DateOnlyRuleWitness {
            version: version(rule.id(), rule.revision())?,
            source: id(rule.source().get())?,
            known_at: millis(rule.known_at())?,
            source_sha256: hash(rule.checksum())?,
            calendar: calendar_version.clone(),
        },
        calendar: EvidenceReference {
            kind: EvidenceReferenceKind::Calendar,
            version: calendar_version,
            source: id(calendar.source().get())?,
            known_at: millis(calendar.known_at())?,
            source_sha256: hash(calendar.checksum())?,
            date: Some(date),
        },
        timezone: DateOnlyTimezone::AmericaNewYork,
        tzdata_revision: calendar.value().timezone().1.into(),
    })
}
