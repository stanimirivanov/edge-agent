use edgeagent_contracts::evidence::schemas::EvidenceSchema;
use edgeagent_contracts::evidence::*;
use edgeagent_contracts::{Component, MessageEnvelope};
use edgeagent_event_spine_conformance::evidence::{artifacts, fixture, verify};
use serde_json::{Value, json};
use std::path::Path;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join("contracts")
}

#[test]
fn generated_evidence_artifacts_have_exact_inventory_and_immutable_baselines() -> Result {
    verify(&root())?;
    let generated = artifacts()?;
    assert_eq!(generated.len(), 13);
    // Independent published SHA-256 census, populated on initial v1 publication.
    let baseline = include_str!("../fixtures/evidence-baselines.txt");
    let mut covered = std::collections::BTreeSet::new();
    for line in baseline.lines() {
        let (expected, path) = line.split_once(' ').ok_or("baseline")?;
        assert!(covered.insert(path), "duplicate baseline path");
        let bytes = generated.get(path).ok_or("uncovered baseline")?;
        assert_eq!(DailyEvidenceArtifact::hash(bytes).as_str(), expected);
    }
    assert_eq!(baseline.lines().count(), generated.len());
    assert_eq!(covered, generated.keys().map(String::as_str).collect());
    Ok(())
}

#[test]
fn canonical_evidence_bytes_preserve_exact_values_times_and_scoped_identity() -> Result {
    let accepted = edgeagent_event_spine_conformance::evidence::native_daily_fixture()?;
    assert!(
        jsonschema::validator_for(&EvidenceSchema::NativeDaily.generate()?)?
            .is_valid(&serde_json::from_slice::<Value>(accepted.bytes())?)
    );
    assert!(matches!(
        DailyObject::decode(accepted.bytes(), accepted.digest())?,
        DailyObject::NativeAccepted(_)
    ));
    assert!(DailyEvidenceArtifact::decode(accepted.bytes(), accepted.digest()).is_err());
    assert!(DateOnlyDailyArtifact::decode(accepted.bytes(), accepted.digest()).is_err());
    let capture = edgeagent_event_spine_conformance::evidence::date_only_daily_fixture()?;
    assert!(
        jsonschema::validator_for(&EvidenceSchema::DateOnlyDaily.generate()?)?
            .is_valid(&serde_json::from_slice::<Value>(capture.bytes())?)
    );
    assert!(matches!(
        DailyObject::decode(capture.bytes(), capture.digest())?,
        DailyObject::DateOnly(_)
    ));
    assert!(DailyEvidenceArtifact::decode(capture.bytes(), capture.digest()).is_err());
    let native = edgeagent_event_spine_conformance::evidence::date_only_fixture()?;
    let validator = jsonschema::validator_for(&EvidenceSchema::DateOnlyTime.generate()?)?;
    let native_json: Value = serde_json::from_slice(native.bytes())?;
    assert!(validator.is_valid(&native_json));
    assert!(
        !jsonschema::validator_for(&EvidenceSchema::Artifact.generate()?)?.is_valid(&native_json)
    );
    assert_eq!(
        DateOnlyTimeEvidence::decode(native.bytes(), native.digest())?,
        native
    );
    let original = fixture()?;
    assert_eq!(
        DailyEvidenceArtifact::decode(original.bytes(), original.digest())?,
        original
    );
    let mut input = original.data().clone();
    input.references.reverse();
    assert_eq!(DailyEvidenceArtifact::new(input)?, original);
    let mut input = original.data().clone();
    input.scope.storage = EvidenceId::new(&"2".repeat(32))?;
    assert_ne!(
        DailyEvidenceArtifact::new(input)?.digest(),
        original.digest()
    );
    let mut input = original.data().clone();
    for price in &mut input.bars[0].ohlc {
        price.coefficient = "9999999999999999999999999999".into();
        price.scale = 0;
    }
    input.bars[0].received_at.nanos = 123;
    input.bars[0].sequence = Some(EvidenceSequence::new("18446744073709551615")?);
    input.cutoff.seconds += 1;
    input.captured_at = input.cutoff;
    let exact = DailyEvidenceArtifact::new(input)?;
    let restored = DailyEvidenceArtifact::decode(exact.bytes(), exact.digest())?;
    assert_eq!(restored.data().bars[0].received_at.nanos, 123);
    assert_eq!(
        restored.data().bars[0]
            .sequence
            .as_ref()
            .map(EvidenceSequence::as_str),
        Some("18446744073709551615")
    );
    assert!(EvidenceSequence::new("0").is_ok());
    assert!(EvidenceSequence::new("00").is_err());
    assert_eq!(
        restored.data().bars[0].ohlc[0].coefficient,
        "9999999999999999999999999999"
    );
    assert!(!format!("{exact:?} {:?}", exact.data()).contains("999999999999"));
    Ok(())
}

#[test]
fn malformed_units_knowledge_coverage_and_lineage_cannot_form_artifacts() -> Result {
    let original = serde_json::to_value(fixture()?.data())?;
    for (pointer, invalid) in [
        ("/bars/0/ohlc/0/coefficient", json!(1.25)),
        ("/bars/0/ohlc/0/coefficient", json!("0")),
        ("/bars/0/ohlc/0/coefficient", json!("01")),
        ("/bars/0/ohlc/0/coefficient", json!("-1")),
        ("/bars/0/ohlc/0/scale", json!(9)),
        ("/bars/0/volume/scale", json!(7)),
        (
            "/bars/0/observation/revision",
            json!("18446744073709551616"),
        ),
        ("/bars/0/predecessor", json!("1")),
        ("/bars/0/sequence", json!("18446744073709551616")),
        ("/bars/0/conditions", json!([2, 1])),
        ("/bars/0/received_at/seconds", json!(172801)),
        ("/bars/0/bucket/1/seconds", json!(259200)),
        ("/cutoff/nanos", json!(1000000000)),
        ("/captured_at/seconds", json!(172799)),
        ("/delete_by/seconds", json!(259201)),
        ("/scope/instrument", json!("0".repeat(32))),
        ("/version", json!("edgeagent.evidence.daily.v2")),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).ok_or("missing pointer")? = invalid;
        assert!(
            serde_json::from_value::<DailyEvidenceArtifact>(changed).is_err(),
            "{pointer}"
        );
    }
    let mut data = fixture()?.data().clone();
    data.bars.push(data.bars[0].clone());
    assert!(DailyEvidenceArtifact::new(data).is_err());
    let mut data = fixture()?.data().clone();
    data.references
        .retain(|r| r.kind != EvidenceReferenceKind::Calendar);
    assert!(DailyEvidenceArtifact::new(data).is_err());
    let mut data = fixture()?.data().clone();
    data.references.push(data.references[0].clone());
    assert!(DailyEvidenceArtifact::new(data).is_err());
    Ok(())
}

#[test]
fn bounded_canonical_decode_rejects_tampering_duplicates_and_payload_diagnostics() -> Result {
    let original = fixture()?;
    assert!(
        DailyEvidenceArtifact::decode(&vec![b' '; MAX_EVIDENCE_BYTES + 1], original.digest())
            .is_err()
    );
    assert!(
        DailyEvidenceArtifact::decode(original.bytes(), &EvidenceDigest::new(&"0".repeat(64))?)
            .is_err()
    );
    let text = std::str::from_utf8(original.bytes())?;
    for changed in [
        format!(" {text}"),
        format!("{text}\n"),
        text.replacen(
            "\"version\":",
            "\"unknown\":\"SENTINEL-SECRET\",\"version\":",
            1,
        ),
        text.replacen(
            "\"version\":",
            "\"version\":\"edgeagent.evidence.daily.v1\",\"version\":",
            1,
        ),
    ] {
        let digest = DailyEvidenceArtifact::hash(changed.as_bytes());
        let error = DailyEvidenceArtifact::decode(changed.as_bytes(), &digest)
            .err()
            .ok_or("tampered bytes accepted")?;
        assert!(!format!("{error:?} {error}").contains("SENTINEL"));
        assert!(!format!("{error}").contains("123456789"));
    }
    Ok(())
}

#[test]
fn evidence_routes_bind_schema_owner_partition_and_reference_only_outcomes() -> Result {
    registry()?;
    let files = artifacts()?;
    for (name, index, kind) in [
        ("acquire", 0, EvidenceSchema::Acquire),
        ("ready", 1, EvidenceSchema::Decision),
        ("rejected", 1, EvidenceSchema::Decision),
    ] {
        let bytes = files
            .get(&format!("fixtures/evidence-v1/{name}.json"))
            .ok_or("fixture missing")?;
        let message = MessageEnvelope::from_json(bytes)?;
        DEFINITIONS[index].validate_envelope(&message)?;
        let payload: Value = message.payload()?;
        assert!(jsonschema::validator_for(&kind.generate()?)?.is_valid(&payload));
        if index == 0 {
            AcquireEvidence::from_envelope(&message)?;
        } else {
            EvidenceDecision::from_envelope(&message)?;
        }
        let text = std::str::from_utf8(bytes)?;
        for forbidden in ["ohlc", "123456789", "https://", "coefficient"] {
            assert!(!text.contains(forbidden));
        }
        let mut changed: Value = serde_json::from_slice(bytes)?;
        changed["partitionkey"] = json!(format!("acquisition/{}", "2".repeat(32)));
        let wrong = MessageEnvelope::from_json(&serde_json::to_vec(&changed)?)?;
        if index == 0 {
            assert!(AcquireEvidence::from_envelope(&wrong).is_err());
        } else {
            assert!(EvidenceDecision::from_envelope(&wrong).is_err());
        }
        if index == 1 {
            changed["partitionkey"] = json!(format!("acquisition/{}", "1".repeat(32)));
            changed["source"] = json!(Component::Gateway.source_uri());
            assert!(
                EvidenceDecision::from_envelope(&MessageEnvelope::from_json(&serde_json::to_vec(
                    &changed
                )?)?)
                .is_err()
            );
        }
    }
    assert_eq!(
        DEFINITIONS[0].message_type(),
        "com.edgeagent.market-data.acquire-evidence.v1"
    );
    assert_eq!(
        DEFINITIONS[1].message_type(),
        "com.edgeagent.market-data.evidence-acquisition-decided.v1"
    );
    Ok(())
}

#[test]
fn local_schemas_reject_version_drift_unknown_fields_and_missing_explicit_nulls() -> Result {
    let artifact = fixture()?;
    let data = serde_json::to_value(artifact.data())?;
    let validator = jsonschema::validator_for(&EvidenceSchema::Artifact.generate()?)?;
    assert!(validator.is_valid(&data));
    let mut changed = data.clone();
    changed["actor"] = json!("forged");
    assert!(!validator.is_valid(&changed));
    assert!(serde_json::from_value::<DailyEvidenceArtifact>(changed).is_err());
    let mut changed = data;
    changed.as_object_mut().ok_or("object")?.remove("replaces");
    assert!(!validator.is_valid(&changed));
    assert!(serde_json::from_value::<DailyEvidenceArtifact>(changed).is_err());
    let command = AcquireEvidence {
        request: artifact.data().scope.request.clone(),
        request_sha256: artifact.data().scope.request_sha256.clone(),
    };
    let mut value = serde_json::to_value(&command)?;
    value["price"] = json!(1);
    assert!(!jsonschema::validator_for(&EvidenceSchema::Acquire.generate()?)?.is_valid(&value));
    assert!(serde_json::from_value::<AcquireEvidence>(value).is_err());
    let decision = EvidenceDecision::ready(command.clone(), &artifact)?;
    let mut value = serde_json::to_value(decision)?;
    value["outcome"]["object"]["bytes"] = json!(0);
    assert!(!jsonschema::validator_for(&EvidenceSchema::Decision.generate()?)?.is_valid(&value));
    assert!(serde_json::from_value::<EvidenceDecision>(value).is_err());
    let mut other = command;
    other.request_sha256 = EvidenceDigest::new(&"3".repeat(64))?;
    assert!(EvidenceDecision::ready(other, &artifact).is_err());
    Ok(())
}
