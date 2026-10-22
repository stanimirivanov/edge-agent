//! Invented v1 evidence fixtures and finite artifact census, not provider qualification.
use edgeagent_contracts::evidence::schemas::EvidenceSchema;
use edgeagent_contracts::evidence::*;
use edgeagent_contracts::{Component, MessageEnvelope, MessageMetadata};
use std::{collections::BTreeMap, path::Path};

/// Build a public synthetic object. No real calendar, prices, rights or authority are asserted.
pub fn fixture() -> Result<DailyEvidenceArtifact, EvidenceContractError> {
    let id = || EvidenceId::new(&"1".repeat(32));
    let version = || {
        Ok::<_, EvidenceContractError>(EvidenceVersion {
            id: id()?,
            revision: EvidenceRevision::new("1")?,
        })
    };
    let date = EvidenceDate::new("1970-01-01")?;
    let time = |seconds| EvidenceTime { seconds, nanos: 0 };
    let mut refs = Vec::new();
    for kind in [
        EvidenceReferenceKind::Mapping,
        EvidenceReferenceKind::Calendar,
        EvidenceReferenceKind::ActionCoverage,
        EvidenceReferenceKind::Rights,
        EvidenceReferenceKind::ProviderReview,
    ] {
        refs.push(EvidenceReference {
            kind,
            version: version()?,
            source: id()?,
            known_at: time(0),
            source_sha256: EvidenceDigest::new(&"7".repeat(64))?,
            date: if kind == EvidenceReferenceKind::Calendar {
                Some(date.clone())
            } else {
                None
            },
        });
    }
    let price = EvidenceDecimal {
        coefficient: "123456789".into(),
        scale: 4,
    };
    DailyEvidenceArtifact::new(DailyEvidence {
        version: EvidenceFormat::V1,
        scope: EvidenceScope {
            request: id()?,
            request_sha256: EvidenceDigest::new(&"2".repeat(64))?,
            principal: id()?,
            account: id()?,
            source: id()?,
            instrument: id()?,
            listing: id()?,
            audience: id()?,
            storage: id()?,
            endpoint: id()?,
        },
        semantics: version()?,
        range: [date.clone(), EvidenceDate::new("1970-01-02")?],
        knowledge: EvidenceKnowledge::Capture,
        cutoff: time(172800),
        captured_at: time(172800),
        delete_by: time(259200),
        quality: EvidenceQuality {
            revision: EvidenceRevision::new("1")?,
            reference_age_ms: EvidenceAgeMillis::new("259200000")?,
            close_change_bps: 1000,
            source_difference_bps: 100,
        },
        references: refs,
        bars: vec![EvidenceBar {
            date,
            observation: version()?,
            predecessor: None,
            transformation: EvidenceRevision::new("1")?,
            sequence: None,
            bucket: [time(0), time(86400)],
            ohlc: [price.clone(), price.clone(), price.clone(), price],
            volume: EvidenceDecimal {
                coefficient: "0".into(),
                scale: 0,
            },
            event_at: time(0),
            received_at: time(172800),
            observed_at: None,
            published_at: None,
            conditions: vec![],
            mapping: version()?,
            calendar: version()?,
            actions: vec![],
        }],
        replaces: None,
    })
}
/// Exact metadata for invented command/fact envelopes, supplied without clock/random work.
pub fn envelope<T: serde::Serialize>(
    index: usize,
    payload: &T,
) -> Result<MessageEnvelope, Box<dyn std::error::Error>> {
    let definition = *DEFINITIONS
        .get(index)
        .ok_or("unknown evidence fixture route")?;
    let metadata = MessageMetadata {
        id: "evidence-fixture".into(),
        source: if index == 0 {
            Component::Gateway.source_uri()
        } else {
            Component::MarketData.source_uri()
        }
        .into(),
        message_type: definition.message_type().into(),
        subject: "acquisition-fixture".into(),
        time: "1970-01-03T00:00:00Z".into(),
        data_schema: definition.data_schema().into(),
        correlation_id: "evidence-fixture".into(),
        causation_id: "evidence-input".into(),
        idempotency_key: "evidence-fixture".into(),
        partition_key: format!("acquisition/{}", "1".repeat(32)),
        trace_parent: "00-11111111111111111111111111111111-2222222222222222-01".into(),
        trace_state: None,
    };
    let value = MessageEnvelope::from_payload(metadata, payload)?;
    definition.validate_envelope(&value)?;
    Ok(value)
}
/// Generate the complete finite current schema/golden census, with repository-relative paths.
pub fn artifacts() -> Result<BTreeMap<String, Vec<u8>>, Box<dyn std::error::Error>> {
    let mut files = BTreeMap::new();
    for schema in EvidenceSchema::ALL {
        files.insert(
            format!("schemas/evidence/{}", schema.filename()),
            schema.artifact()?.into_bytes(),
        );
    }
    let artifact = fixture()?;
    let request = AcquireEvidence {
        request: artifact.data().scope.request.clone(),
        request_sha256: artifact.data().scope.request_sha256.clone(),
    };
    let ready = EvidenceDecision::ready(request.clone(), &artifact)?;
    let rejected = EvidenceDecision::rejected(request.clone(), EvidenceRejection::Quality);
    files.insert(
        "fixtures/evidence-v1/daily.json".into(),
        artifact.bytes().to_vec(),
    );
    for (name, index, payload) in [
        ("acquire", 0, serde_json::to_value(request)?),
        ("ready", 1, serde_json::to_value(ready)?),
        ("rejected", 1, serde_json::to_value(rejected)?),
    ] {
        let mut bytes = envelope(index, &payload)?.to_json()?;
        bytes.push(b'\n');
        files.insert(format!("fixtures/evidence-v1/{name}.json"), bytes);
    }
    Ok(files)
}
/// Verify byte freshness, exact inventories and regular files; never resolve schema URIs.
pub fn verify(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let files = artifacts()?;
    for directory in ["schemas/evidence", "fixtures/evidence-v1"] {
        let path = root.join(directory);
        if !std::fs::symlink_metadata(&path)?.file_type().is_dir() {
            return Err("evidence inventory is not a regular directory".into());
        }
        let mut actual = std::collections::BTreeSet::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Err("nonregular evidence artifact".into());
            }
            actual.insert(format!(
                "{directory}/{}",
                entry.file_name().to_str().ok_or("invalid filename")?
            ));
        }
        let expected = files
            .keys()
            .filter(|k| k.starts_with(&format!("{directory}/")))
            .cloned()
            .collect();
        if actual != expected {
            return Err("evidence artifact census differs".into());
        }
    }
    for (path, bytes) in files {
        if std::fs::read(root.join(path))? != bytes {
            return Err("stale evidence artifact; regenerate and review compatibility".into());
        }
    }
    Ok(())
}
