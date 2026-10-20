//! Golden synthetic admission DTO/schema boundary, independent of PostgreSQL.
use edgeagent_contracts::simulation::{
    AdmissionStatus, DEFINITIONS, OUTCOME_SCHEMA, OrderAdmission, SUBMIT_SCHEMA, SubmitDryRunOrder,
};
use edgeagent_event_spine_conformance::accounts::{Error, Scenario};
use serde_json::{Value, json};

#[test]
fn published_admission_schemas_retain_immutable_content_and_identity() {
    for (schema, expected) in [
        (
            SUBMIT_SCHEMA,
            "8472a91857e08307a069baced90d9769e26066904a4130cd59250d7443368850",
        ),
        (
            OUTCOME_SCHEMA,
            "ccea6de274ffd314728827bdc2789b4c99eeeea332a3a8636899ede8d0bbf443",
        ),
    ] {
        let normalized = schema.replace("\r\n", "\n");
        let digest = edgeagent_simulation_domain::ArtifactDigest::of(normalized.as_bytes());
        assert_eq!(
            edgeagent_simulation_postgres::hex(&digest.bytes()),
            expected,
            "Published schema bytes are immutable; introduce a new version instead of changing v1"
        );
    }
}

#[test]
fn admission_wire_schema_and_constructor_reject_unsupported_modes_and_invalid_facts()
-> Result<(), Error> {
    let scenario = Scenario::new("cash")?;
    let request = scenario.request(0)?;
    DEFINITIONS[0].validate_envelope(&scenario.envelope(0, "wire-fixture")?)?;
    let submit: Value = serde_json::from_str(SUBMIT_SCHEMA)?;
    let validator = jsonschema::validator_for(&submit)?;
    let command = serde_json::to_value(&request)?;
    assert!(validator.is_valid(&command));
    assert_eq!(
        serde_json::from_value::<SubmitDryRunOrder>(command.clone())?,
        request
    );
    for (field, value) in [
        ("mode", json!("live")),
        ("mode", Value::Null),
        ("account", json!("0".repeat(32))),
        ("order", json!("A".repeat(64))),
        ("quote", json!("short")),
        ("input_revision", json!(0)),
        ("expected_revision", json!(i64::MAX)),
    ] {
        let mut invalid = command.clone();
        invalid[field] = value;
        assert!(!validator.is_valid(&invalid));
        assert!(serde_json::from_value::<SubmitDryRunOrder>(invalid).is_err());
    }
    let outcome = OrderAdmission::new(
        &request,
        &"1".repeat(64),
        &"1".repeat(32),
        1,
        AdmissionStatus::Admitted,
        (6000, 0),
        (3, None),
    )?;
    let mut schema: Value = serde_json::from_str(OUTCOME_SCHEMA)?;
    // Resolve only this known local schema. The test never enables URI fetching.
    schema["properties"]["request"] = submit;
    let validator = jsonschema::validator_for(&schema)?;
    let fact = serde_json::to_value(&outcome)?;
    assert!(validator.is_valid(&fact));
    assert_eq!(
        serde_json::from_value::<OrderAdmission>(fact.clone())?,
        outcome
    );
    for (field, value) in [
        ("status", json!("live")),
        ("risk_reason", json!("account_gross")),
        ("risk_revision", json!(0)),
        ("order_revision", json!(2)),
    ] {
        let mut invalid = fact.clone();
        invalid[field] = value;
        assert!(!validator.is_valid(&invalid));
        assert!(serde_json::from_value::<OrderAdmission>(invalid).is_err());
    }
    let mut invalid = fact;
    invalid["status"] = json!("insufficient_cash");
    assert!(!validator.is_valid(&invalid));
    assert!(serde_json::from_value::<OrderAdmission>(invalid).is_err());
    assert!(
        OrderAdmission::new(
            &request,
            &"1".repeat(64),
            &"1".repeat(32),
            2,
            AdmissionStatus::Admitted,
            (1, 0),
            (3, None)
        )
        .is_err()
    );
    assert!(!format!("{outcome:?}").contains("6000"));
    assert_eq!(
        DEFINITIONS[0].data_schema(),
        "urn:edgeagent:schema:simulation:submit-order:v1"
    );
    assert_eq!(
        DEFINITIONS[1].message_type(),
        "com.edgeagent.simulation.order-admission-decided.v1"
    );
    Ok(())
}
