use edgeagent_contracts::strategy::{Configuration, MAX_BUNDLE_BYTES};
use serde_json::{Value, json};
type Result = std::result::Result<(), Box<dyn std::error::Error>>;
const BYTES: &[u8] = include_bytes!("../fixtures/strategy/configuration-v1.json");

#[test]
fn frozen_bundle_bytes_and_identity_are_stable() -> Result {
    let bundle = Configuration::decode(BYTES)?;
    assert_eq!(bundle.canonical_bytes()?, BYTES);
    assert_eq!(
        bundle.digest()?,
        "15abc53d1833423e57071cb99e857c2ab8955b7fc880c80f62e7d7ce3f9e9c58"
    );
    let mut changed: Value = serde_json::from_slice(BYTES)?;
    changed["data"]["strategy"]["expiry_ms"] = json!(1001);
    let changed: Configuration = serde_json::from_value(changed)?;
    assert_ne!(bundle.digest()?, changed.digest()?);
    Ok(())
}
#[test]
fn unknown_fields_versions_and_incompatible_declarations_reject() -> Result {
    let base: Value = serde_json::from_slice(BYTES)?;
    for pointer in ["", "/data", "/data/strategy", "/data/policy"] {
        let mut v = base.clone();
        v.pointer_mut(pointer).ok_or("missing fixture node")?["unknown"] =
            json!("PRIVATE_SENTINEL");
        assert!(serde_json::from_value::<Configuration>(v).is_err());
    }
    for (pointer, bad) in [
        ("/kind", json!("research.configuration.v2")),
        ("/data/compatibility", json!("research_v2")),
        ("/data/strategy/ranking", json!("floating_score")),
        ("/data/policy/direction", json!("short")),
    ] {
        let mut v = base.clone();
        *v.pointer_mut(pointer).ok_or("missing node")? = bad;
        assert!(serde_json::from_value::<Configuration>(v).is_err());
    }
    for (pointer, bad) in [
        ("/data/strategy/lookback_sessions", json!(0)),
        ("/data/strategy/horizon_sessions", json!(6)),
        ("/data/strategy/expiry_ms", json!(300001)),
        ("/data/policy/max_age_ms", json!(0)),
        ("/data/policy/instruments", json!(["b", "a"])),
        ("/data/policy/instruments", json!(["a", "a"])),
        ("/data/strategy/required_features", json!(["closes"])),
        (
            "/data/strategy/required_features",
            json!(["closes", "closes", "volume", "event_coverage"]),
        ),
        ("/data/strategy/implementation", json!("0".repeat(64))),
        ("/data/policy/revision", json!("A".repeat(64))),
        ("/data/strategy/regime_revision", json!("")),
    ] {
        let mut v = base.clone();
        *v.pointer_mut(pointer).ok_or("missing node")? = bad;
        let bundle: Configuration = serde_json::from_value(v)?;
        assert!(bundle.canonical_bytes().is_err(), "{pointer}");
    }
    Ok(())
}
#[test]
fn raw_boundary_rejects_duplicates_alternate_encoding_and_oversize_safely() -> Result {
    let text = std::str::from_utf8(BYTES)?;
    for bad in [
        text.replace(
            "\"expiry_ms\":1000",
            "\"expiry_ms\":1000,\"expiry_ms\":1000",
        ),
        format!(" {text}"),
        text.trim_end().to_owned(),
        text.replace("research.configuration.v1", "PRIVATE_SENTINEL"),
        "x".repeat(MAX_BUNDLE_BYTES + 1),
    ] {
        let error = Configuration::decode(bad.as_bytes())
            .err()
            .ok_or("expected refusal")?;
        assert_eq!(error.to_string(), "invalid strategy configuration");
        assert_eq!(format!("{error:?}"), "ConfigurationError");
    }
    Ok(())
}
#[test]
fn explicit_zero_constraints_and_inclusive_bounds_have_no_defaults() -> Result {
    let mut v: Value = serde_json::from_slice(BYTES)?;
    v["data"]["strategy"]["lookback_sessions"] = json!(252);
    v["data"]["strategy"]["expiry_ms"] = json!(300000);
    v["data"]["policy"]["max_age_ms"] = json!(2678400000_u32);
    let bundle: Configuration = serde_json::from_value(v.clone())?;
    assert!(Configuration::decode(&bundle.canonical_bytes()?).is_ok());
    v["data"]["policy"]
        .as_object_mut()
        .ok_or("object")?
        .remove("max_gap_ppm");
    assert!(serde_json::from_value::<Configuration>(v).is_err());
    Ok(())
}
