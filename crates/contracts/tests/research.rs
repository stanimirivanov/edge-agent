use edgeagent_contracts::research::*;
use serde_json::Value;
type TestResult = Result<(), Box<dyn std::error::Error>>;
const FIXTURES: [&[u8]; 4] = [
    include_bytes!("../fixtures/research/v1/request.json"),
    include_bytes!("../fixtures/research/v1/artifact.json"),
    include_bytes!("../fixtures/research/v1/abstention.json"),
    include_bytes!("../fixtures/research/v1/lifecycle.json"),
];
#[test]
fn frozen_records_round_trip_and_bind_exact_content() -> TestResult {
    for bytes in FIXTURES {
        let record = ResearchRecord::decode(bytes)?;
        assert_eq!(record.canonical_bytes()?, bytes);
        assert_eq!(record.digest()?.len(), 64);
        assert!(record == serde_json::from_slice(bytes)?);
    }
    Ok(())
}
fn reject(value: Value) -> TestResult {
    let encoded = serde_json::to_vec(&value)?;
    assert!(ResearchRecord::decode(&encoded).is_err());
    // Constructor/serialization boundary must reject semantic failures too.
    if let Ok(raw) = serde_json::from_value::<ResearchRecord>(value) {
        assert!(raw.canonical_bytes().is_err());
    }
    Ok(())
}
#[test]
fn invalid_shapes_versions_bounds_and_semantics_are_rejected() -> TestResult {
    let request: Value = serde_json::from_slice(FIXTURES[0])?;
    for (pointer, value) in [
        ("/kind", "research.request.v2".into()),
        ("/data/request_id", "private sentinel!".into()),
        ("/data/lookback_sessions", 0.into()),
        ("/data/decision_at_ms", (-1).into()),
        ("/data/configuration", "bad".into()),
        ("/data/instruments", serde_json::json!(["B", "A"])),
    ] {
        let mut v = request.clone();
        *v.pointer_mut(pointer).ok_or("missing fixture field")? = value;
        reject(v)?;
    }
    let mut extra = request.clone();
    extra["data"]["unknown"] = true.into();
    reject(extra)?;
    let mut extra = request;
    extra["unknown"] = true.into();
    reject(extra)?;
    let artifact: Value = serde_json::from_slice(FIXTURES[1])?;
    for (pointer, value) in [
        ("/data/expires_at_ms", 0.into()),
        ("/data/evidence/0/available_at_ms", 999.into()),
        ("/data/ranking/0/instrument", "FOREIGN".into()),
        ("/data/explanation", "x".repeat(4097).into()),
        ("/data/scenarios", Value::Null),
    ] {
        let mut v = artifact.clone();
        *v.pointer_mut(pointer).ok_or("missing fixture field")? = value;
        reject(v)?;
    }
    let mut v = artifact;
    v["data"]["ranking"] = serde_json::json!([]);
    reject(v)?;
    let mut v: Value = serde_json::from_slice(FIXTURES[2])?;
    v["data"]["reason"] = "invented".into();
    reject(v)?;
    let mut v: Value = serde_json::from_slice(FIXTURES[3])?;
    v["data"]["revision"] = 0.into();
    reject(v)?;
    Ok(())
}
#[test]
fn raw_boundaries_reject_duplicates_noncanonical_and_oversize_without_leaking() -> TestResult {
    let bytes = FIXTURES[0];
    let mut whitespace = bytes.to_vec();
    whitespace.push(b'\n');
    let oversized = vec![b'x'; MAX_RESEARCH_BYTES + 1];
    let duplicate = String::from_utf8(bytes.to_vec())?.replacen(
        "\"scope\":",
        "\"scope\":\"private-sentinel\",\"scope\":",
        1,
    );
    for invalid in [
        &whitespace[..],
        &oversized,
        duplicate.as_bytes(),
        b"private-sentinel",
    ] {
        let error = ResearchRecord::decode(invalid)
            .err()
            .ok_or("expected rejection")?;
        assert_eq!(error.to_string(), "invalid research v1 contract");
        assert!(!format!("{error:?}").contains("private-sentinel"));
    }
    Ok(())
}
#[cfg(feature = "schema")]
#[test]
fn published_schema_matches_frozen_v1_inventory() -> TestResult {
    assert_eq!(
        schemas::generate()?,
        include_str!("../fixtures/research/v1/schema.json")
    );
    Ok(())
}

#[test]
fn lifecycle_reasons_and_candidate_order_are_stable() -> TestResult {
    let mut value: Value = serde_json::from_slice(FIXTURES[3])?;
    for transition in ["invalidated", "withdrawn", "expired"] {
        value["data"]["change"] = serde_json::json!({"transition": transition});
        let record: ResearchRecord = serde_json::from_value(value.clone())?;
        assert!(ResearchRecord::decode(&record.canonical_bytes()?).is_ok());
    }
    value["data"]["change"] =
        serde_json::json!({"transition":"corrected", "replacement":"d".repeat(64)});
    reject(value.clone())?;
    value["data"]["change"]["replacement"] = "e".repeat(64).into();
    assert!(
        serde_json::from_value::<ResearchRecord>(value)?
            .canonical_bytes()
            .is_ok()
    );
    let mut artifact: Value = serde_json::from_slice(FIXTURES[1])?;
    artifact["data"]["ranking"] =
        serde_json::json!([{"instrument":"AAA","score_ppm":1},{"instrument":"AAA","score_ppm":1}]);
    reject(artifact)?;
    Ok(())
}
