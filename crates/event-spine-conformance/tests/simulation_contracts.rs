//! Golden synthetic admission DTO/schema boundary, independent of PostgreSQL.
use edgeagent_contracts::simulation::{
    AdmissionStatus, DEFINITIONS, OUTCOME_SCHEMA, OrderAdmission, SUBMIT_SCHEMA, SubmitDryRunOrder,
};
use edgeagent_event_spine_conformance::accounts::{Error, Scenario};
use serde_json::{Value, json};

#[test]
fn published_settlement_schemas_retain_immutable_content_and_identity() {
    use edgeagent_contracts::settlement::{COMMAND_SCHEMA, DECISION_SCHEMA};
    for (schema, expected) in [
        (
            COMMAND_SCHEMA,
            "ded057a7a4b4006cd47558e3a9a18df42ec8128c3ef0cbc295b9e4ad486b322b",
        ),
        (
            DECISION_SCHEMA,
            "5f90b18ef94b71547d614b0a1978937a0759c51b1cb6f3f1db3d736df746bd25",
        ),
    ] {
        let normalized = schema.replace("\r\n", "\n");
        assert_eq!(
            edgeagent_simulation_postgres::hex(
                &edgeagent_simulation_domain::ArtifactDigest::of(normalized.as_bytes()).bytes()
            ),
            expected
        );
    }
}

#[test]
fn settlement_wire_contracts_validate_balanced_and_bounded_committed_facts() -> Result<(), Error> {
    use edgeagent_contracts::settlement::*;
    let scenario = Scenario::new("execution-settlement-call-itm")?;
    let owner = scenario.context(1)?;
    let execution = scenario.execution(&owner)?;
    let settlement = scenario.settlement(&execution)?;
    let envelope = scenario.settlement_envelope(&settlement, 8, "wire-settlement", false)?;
    DEFINITIONS[0].validate_envelope(&envelope)?;
    let command: SettleDryRunAccount = envelope.payload()?;
    let value = serde_json::to_value(&command)?;
    let command_schema: Value = serde_json::from_str(COMMAND_SCHEMA)?;
    let command_validator = jsonschema::validator_for(&command_schema)?;
    assert!(command_validator.is_valid(&value));
    for (field, invalid) in [
        ("mode", json!("live")),
        ("account", json!("0".repeat(32))),
        ("sequence", json!(0)),
        ("cash", json!(10)),
        ("evidence", json!("invented")),
        ("actor", json!(1)),
    ] {
        let mut changed = value.clone();
        changed[field] = invalid;
        assert!(!command_validator.is_valid(&changed));
        assert!(serde_json::from_value::<SettleDryRunAccount>(changed).is_err());
    }
    let allocation = SettlementAllocation::new(&"1".repeat(32), 3, 2, 3, [-1, -18_000, 17_998, 3])?;
    let fact = AccountSettlement::new(
        command.clone(),
        &"1".repeat(32),
        "settled",
        (&"2".repeat(64), &"3".repeat(64), Some(&"4".repeat(64))),
        vec![allocation],
        vec![("5".repeat(64), 2)],
    )?;
    let value = serde_json::to_value(&fact)?;
    let mut schema: Value = serde_json::from_str(DECISION_SCHEMA)?;
    schema["properties"]["request"] = command_schema;
    let validator = jsonschema::validator_for(&schema)?;
    assert!(validator.is_valid(&value));
    assert_eq!(
        serde_json::from_value::<AccountSettlement>(value.clone())?,
        fact
    );
    let mut unbalanced = value.clone();
    unbalanced["allocations"][0]["postings"][0] = json!(0);
    assert!(serde_json::from_value::<AccountSettlement>(unbalanced).is_err());
    for (field, invalid) in [
        ("status", json!("live")),
        ("evidence", Value::Null),
        ("account_revision", json!(0)),
        ("actor", json!(1)),
        ("instrument", json!("0".repeat(32))),
    ] {
        let mut changed = value.clone();
        changed[field] = invalid;
        assert!(!validator.is_valid(&changed));
        assert!(serde_json::from_value::<AccountSettlement>(changed).is_err());
    }
    let deferred = AccountSettlement::new(
        command,
        &"1".repeat(32),
        "deferred_missing",
        (&"2".repeat(64), &"3".repeat(64), None),
        vec![],
        vec![],
    )?;
    let mut missing = serde_json::to_value(&deferred)?;
    assert!(validator.is_valid(&missing));
    missing.as_object_mut().ok_or("object")?.remove("evidence");
    assert!(!validator.is_valid(&missing));
    assert!(serde_json::from_value::<AccountSettlement>(missing).is_err());
    assert_eq!(format!("{fact:?}"), "AccountSettlement(..)");
    Ok(())
}

#[test]
fn published_execution_schemas_retain_immutable_content_and_identity() {
    use edgeagent_contracts::execution::{DECISION_SCHEMA, STEP_SCHEMA};
    for (schema, expected) in [
        (
            STEP_SCHEMA,
            "9a9f037d507b9b22b62680cfd0ccd343e4b3c3b6ef8cef6480cd4f87c1e70b87",
        ),
        (
            DECISION_SCHEMA,
            "8e5271bad98be9b0b4f742df9ac0c13c39e4db19fc9e8654fff0963a6d83c7a9",
        ),
    ] {
        let normalized = schema.replace("\r\n", "\n");
        assert_eq!(
            edgeagent_simulation_postgres::hex(
                &edgeagent_simulation_domain::ArtifactDigest::of(normalized.as_bytes()).bytes()
            ),
            expected
        );
    }
}
#[test]
fn execution_wire_schema_and_constructor_reject_mutable_financial_input() -> Result<(), Error> {
    use edgeagent_contracts::execution::*;
    let scenario = Scenario::new("execution")?;
    let admission = scenario.context(1)?;
    let context = scenario.execution(&admission)?;
    let envelope = scenario.execution_envelope(&context, 3, "execution-wire", false)?;
    DEFINITIONS[0].validate_envelope(&envelope)?;
    let request: ExecuteDryRunStep = envelope.payload()?;
    let value = serde_json::to_value(&request)?;
    let step: Value = serde_json::from_str(STEP_SCHEMA)?;
    let validator = jsonschema::validator_for(&step)?;
    assert!(validator.is_valid(&value));
    assert_eq!(
        serde_json::from_value::<ExecuteDryRunStep>(value.clone())?,
        request
    );
    for (field, invalid) in [
        ("mode", json!("live")),
        ("account", json!("0".repeat(32))),
        ("plan", json!("A".repeat(64))),
        ("sequence", json!(0)),
        ("expected_revision", json!(i64::MAX)),
        ("cash", json!(100)),
        ("actor", json!(1)),
    ] {
        let mut changed = value.clone();
        changed[field] = invalid;
        assert!(!validator.is_valid(&changed));
        assert!(serde_json::from_value::<ExecuteDryRunStep>(changed).is_err());
    }
    let fact = ExecutionDecision::new(
        request,
        &"1".repeat(64),
        2,
        "no_fill",
        (0, [0; 4]),
        (&"2".repeat(64), Some(&"3".repeat(64))),
        (&"1".repeat(32), &"1".repeat(32), "shares", "buy_to_open"),
    )?;
    let value = serde_json::to_value(&fact)?;
    let mut schema: Value = serde_json::from_str(DECISION_SCHEMA)?;
    schema["properties"]["request"] = step;
    let validator = jsonschema::validator_for(&schema)?;
    assert!(validator.is_valid(&value));
    assert_eq!(
        serde_json::from_value::<ExecutionDecision>(value.clone())?,
        fact
    );
    for (field, invalid) in [
        ("status", json!("live")),
        ("filled_units", json!(1)),
        ("cash_delta_cents", json!(1)),
        ("evidence", Value::Null),
        ("order_revision", json!(0)),
        ("model", json!("invalid")),
        ("actor", json!(1)),
    ] {
        let mut changed = value.clone();
        changed[field] = invalid;
        assert!(!validator.is_valid(&changed));
        assert!(serde_json::from_value::<ExecutionDecision>(changed).is_err());
    }
    assert!(!format!("{fact:?}").contains("no_fill"));
    let mut unbalanced = value.clone();
    unbalanced["status"] = json!("partial_fill");
    unbalanced["filled_units"] = json!(1);
    unbalanced["cash_delta_cents"] = json!(-101);
    unbalanced["position_cost_delta_cents"] = json!(100);
    // JSON Schema checks shapes; the checked DTO additionally enforces arithmetic.
    assert!(validator.is_valid(&unbalanced));
    assert!(serde_json::from_value::<ExecutionDecision>(unbalanced.clone()).is_err());
    unbalanced["fee_delta_cents"] = json!(1);
    let balanced = serde_json::from_value::<ExecutionDecision>(unbalanced)?;
    assert_eq!(balanced.postings().iter().sum::<i128>(), 0);
    assert_eq!(balanced.allocation().2, "shares");
    Ok(())
}

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
