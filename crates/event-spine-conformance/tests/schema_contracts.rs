use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use edgeagent_contracts::{
    Component, MAX_PORTABLE_MESSAGE_BYTES, MessageEnvelope, MessageMetadata,
};
use edgeagent_event_spine_conformance::{
    AcceptanceError, AcceptanceMessage, ApplyProbe, ProbeApplied, definitions, registry,
    schemas::{SchemaKind, canonical_json, verify_artifacts, verify_optional_additions},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error>>;

const COMMAND: &str = include_str!("../fixtures/v1/apply-probe.json");
const FACT: &str = include_str!("../fixtures/v1/probe-applied.json");

fn metadata(kind: SchemaKind) -> Result<MessageMetadata, Box<dyn Error>> {
    let (id, source, cause) = match kind {
        SchemaKind::Command => ("command-01", Component::Gateway, "request-01"),
        SchemaKind::Fact => ("fact-01", Component::Research, "command-01"),
    };
    Ok(MessageMetadata {
        id: id.into(),
        source: source.source_uri().into(),
        message_type: kind.definition().message_type().into(),
        subject: "probe/probe-01".into(),
        time: "2026-10-16T00:00:00Z".into(),
        data_schema: kind.definition().data_schema().into(),
        correlation_id: "correlation-01".into(),
        causation_id: cause.into(),
        idempotency_key: "request-01".into(),
        partition_key: "probe/probe-01".into(),
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
        trace_state: None,
    })
}

fn changed(fixture: &str, field: &str, value: Value) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut message: Value = serde_json::from_str(fixture)?;
    message[field] = value;
    Ok(serde_json::to_vec(&message)?)
}

#[test]
fn generated_schemas_match_the_complete_committed_inventory() -> TestResult {
    verify_artifacts(&Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/v1"))?;
    for kind in SchemaKind::ALL {
        let schema = kind.generate()?;
        assert_eq!(schema["$id"], kind.definition().data_schema());
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert!(!kind.artifact()?.contains("\"$ref\""));
        assert_eq!(canonical_json(schema)?, kind.artifact()?);
    }
    Ok(())
}

#[test]
fn schema_inventory_rejects_stale_missing_extra_and_nonregular_artifacts() -> TestResult {
    let directory = Directory::new()?;
    for kind in SchemaKind::ALL {
        fs::write(directory.path().join(kind.filename()), kind.artifact()?)?;
    }
    verify_artifacts(directory.path())?;
    let command = directory.path().join(SchemaKind::Command.filename());
    fs::write(&command, "{}\n")?;
    assert!(verify_artifacts(directory.path()).is_err());
    fs::remove_file(&command)?;
    assert!(verify_artifacts(directory.path()).is_err());
    fs::write(&command, SchemaKind::Command.artifact()?)?;
    let orphan = directory.path().join("orphan.json");
    fs::write(&orphan, "{}\n")?;
    assert!(verify_artifacts(directory.path()).is_err());
    fs::remove_file(&orphan)?;
    fs::create_dir(directory.path().join("nested"))?;
    assert!(verify_artifacts(directory.path()).is_err());
    Ok(())
}

#[test]
fn retained_baselines_preserve_schema_identity_and_existing_field_constraints() -> TestResult {
    for (kind, baseline) in [
        (
            SchemaKind::Command,
            include_str!("../fixtures/baseline/v1/apply-probe.schema.json"),
        ),
        (
            SchemaKind::Fact,
            include_str!("../fixtures/baseline/v1/probe-applied.schema.json"),
        ),
    ] {
        let baseline: Value = serde_json::from_str(baseline)?;
        let current = kind.generate()?;
        verify_optional_additions(&baseline, &current)?;
        // V1 identities are immutable; a compatible candidate does not grant
        // permission to replace the currently published schema artifact.
        assert_eq!(baseline, current);
        assert_eq!(current["properties"]["probe_id"]["minLength"], 1);
        assert_eq!(current["properties"]["probe_id"]["maxLength"], 64);
        assert_eq!(
            current["properties"]["probe_id"]["pattern"],
            "^[a-z0-9][a-z0-9_-]*$"
        );
        assert_eq!(current["properties"]["value"]["maximum"], 1_000_000);
    }
    Ok(())
}

#[derive(Deserialize, Serialize)]
struct FutureReader {
    #[serde(flatten)]
    command: ApplyProbe,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[test]
fn optional_additions_preserve_old_readers_and_default_new_reader_fields() -> TestResult {
    let baseline = SchemaKind::Command.generate()?;
    let mut candidate = baseline.clone();
    candidate["properties"]["note"] = json!({"type": "string"});
    verify_optional_additions(&baseline, &candidate)?;
    let envelope = MessageEnvelope::from_json(COMMAND.as_bytes())?;
    let original = envelope.payload::<Value>()?;
    let old = serde_json::from_value::<ApplyProbe>(original.clone())?;
    let future = serde_json::from_value::<FutureReader>(original.clone())?;
    assert_eq!(future.command, old);
    assert!(future.note.is_none());
    let mut evolved = original;
    evolved["note"] = json!("synthetic optional field");
    assert!(jsonschema::validator_for(&candidate)?.is_valid(&evolved));
    assert_eq!(serde_json::from_value::<ApplyProbe>(evolved.clone())?, old);
    assert_eq!(
        serde_json::from_value::<FutureReader>(evolved.clone())?
            .note
            .as_deref(),
        Some("synthetic optional field")
    );
    let bytes = changed(COMMAND, "data", evolved)?;
    let envelope = MessageEnvelope::from_json(&bytes)?;
    let before = envelope.to_json()?;
    assert_eq!(
        AcceptanceMessage::from_envelope(&envelope)?,
        AcceptanceMessage::Command(old)
    );
    assert_eq!(envelope.to_json()?, before);
    Ok(())
}

#[test]
fn compatibility_policy_rejects_breaking_and_unclassified_schema_mutations() -> TestResult {
    for kind in SchemaKind::ALL {
        let baseline = kind.generate()?;
        let mut mutations = Vec::new();
        for (pointer, value) in [
            ("/properties/value/type", json!("string")),
            ("/properties/value/maximum", json!(999_999)),
            ("/properties/value/minimum", json!(1)),
            ("/properties/probe_id/pattern", json!("^[a-z]+$")),
            ("/$id", json!("https://untrusted.invalid/schema")),
            ("/type", json!("array")),
        ] {
            let mut candidate = baseline.clone();
            *candidate
                .pointer_mut(pointer)
                .ok_or("mutation target absent")? = value;
            mutations.push(candidate);
        }
        let mut removed = baseline.clone();
        removed["properties"]
            .as_object_mut()
            .ok_or("properties missing")?
            .remove("value");
        mutations.push(removed);
        let mut required = baseline.clone();
        required["properties"]["extra"] = json!({"type": "string"});
        required["required"]
            .as_array_mut()
            .ok_or("required missing")?
            .push(json!("extra"));
        mutations.push(required);
        for (keyword, value) in [
            ("additionalProperties", json!(false)),
            ("dependentRequired", json!({"probe_id": ["extra"]})),
            ("allOf", json!([{"required": ["extra"]}])),
            ("$ref", json!("https://untrusted.invalid/schema")),
        ] {
            let mut candidate = baseline.clone();
            candidate[keyword] = value;
            mutations.push(candidate);
        }
        let mut narrowed = baseline.clone();
        narrowed["properties"]["value"]["enum"] = json!([42]);
        mutations.push(narrowed);
        for candidate in mutations {
            assert_eq!(
                verify_optional_additions(&baseline, &candidate),
                Err(AcceptanceError::Compatibility)
            );
        }
        assert!(verify_optional_additions(&json!(false), &baseline).is_err());
        assert!(verify_optional_additions(&baseline, &json!(null)).is_err());
    }
    Ok(())
}

#[test]
fn typed_command_and_fact_round_trip_exact_golden_envelopes() -> TestResult {
    let command = ApplyProbe::new("probe-01", 0, 42)?;
    let fact = ProbeApplied::new("probe-01", 1, 42)?;
    for (envelope, fixture, expected) in [
        (
            command.envelope(metadata(SchemaKind::Command)?)?,
            COMMAND,
            AcceptanceMessage::Command(command),
        ),
        (
            fact.envelope(metadata(SchemaKind::Fact)?)?,
            FACT,
            AcceptanceMessage::Fact(fact),
        ),
    ] {
        assert_eq!(envelope.to_json()?, fixture.trim_end().as_bytes());
        assert_eq!(AcceptanceMessage::from_json(fixture.as_bytes())?, expected);
        registry()?.validate(&envelope)?;
    }
    assert_eq!(
        definitions()[0].subject()?,
        "edgeagent.command.conformance.apply-probe.v1"
    );
    assert_eq!(
        definitions()[1].subject()?,
        "edgeagent.event.conformance.probe-applied.v1"
    );
    Ok(())
}

#[test]
fn payload_fixture_matrix_rejects_invalid_shapes_and_boundary_values() -> TestResult {
    let cases: Value = serde_json::from_str(include_str!("../fixtures/v1/payload-cases.json"))?;
    for (kind, key, fixture) in [
        (SchemaKind::Command, "command", COMMAND),
        (SchemaKind::Fact, "fact", FACT),
    ] {
        for (group, valid) in [("valid", true), ("invalid", false)] {
            for payload in cases[key][group]
                .as_array()
                .ok_or("fixture group missing")?
            {
                assert_eq!(kind.validate(payload).is_ok(), valid);
                let result =
                    AcceptanceMessage::from_json(&changed(fixture, "data", payload.clone())?);
                assert_eq!(result.is_ok(), valid);
                if !valid {
                    assert_eq!(result, Err(AcceptanceError::Payload));
                }
            }
        }
    }
    assert!(ApplyProbe::new("a".repeat(64), 1_000_000, 1_000_000).is_ok());
    assert!(ApplyProbe::new("a".repeat(65), 0, 0).is_err());
    assert!(ApplyProbe::new("é", 0, 0).is_err());
    assert!(ProbeApplied::new("a".repeat(64), 1_000_001, 1_000_000).is_ok());
    Ok(())
}

#[test]
fn version_schema_producer_and_partition_mismatches_never_yield_typed_messages() -> TestResult {
    for (fixture, field, value, expected) in [
        (
            COMMAND,
            "type",
            json!("com.edgeagent.conformance.apply-probe.v2"),
            AcceptanceError::Routing,
        ),
        (
            COMMAND,
            "dataschema",
            json!("file:///private-schema-sentinel-7391"),
            AcceptanceError::Routing,
        ),
        (
            COMMAND,
            "dataschema",
            json!("https://untrusted.invalid/schema"),
            AcceptanceError::Routing,
        ),
        (
            FACT,
            "source",
            json!(Component::Gateway.source_uri()),
            AcceptanceError::Routing,
        ),
        (
            COMMAND,
            "partitionkey",
            json!("other/probe-01"),
            AcceptanceError::Routing,
        ),
        (
            FACT,
            "partitionkey",
            json!("probe/other"),
            AcceptanceError::Partition,
        ),
    ] {
        assert_eq!(
            AcceptanceMessage::from_json(&changed(fixture, field, value)?),
            Err(expected)
        );
    }
    assert!(
        ApplyProbe::new("probe-01", 0, 42)?
            .envelope(metadata(SchemaKind::Fact)?)
            .is_err()
    );
    assert!(
        ProbeApplied::new("probe-01", 1, 42)?
            .envelope(metadata(SchemaKind::Command)?)
            .is_err()
    );
    let mut wrong = metadata(SchemaKind::Command)?;
    wrong.partition_key = "probe/other".into();
    assert_eq!(
        ApplyProbe::new("probe-01", 0, 42)?.envelope(wrong),
        Err(AcceptanceError::Partition)
    );
    Ok(())
}

#[test]
fn constructor_serde_and_raw_byte_boundaries_cannot_bypass_validation() -> TestResult {
    assert!(ApplyProbe::new("probe-01", 1_000_001, 0).is_err());
    assert!(ProbeApplied::new("probe-01", 0, 0).is_err());
    for bytes in [
        b"not JSON".as_slice(),
        b"{\"data\":\"PRIVATE-sentinel-7391\"}",
    ] {
        assert_eq!(
            AcceptanceMessage::from_json(bytes),
            Err(AcceptanceError::Envelope)
        );
    }
    assert_eq!(
        AcceptanceMessage::from_json(&vec![b' '; MAX_PORTABLE_MESSAGE_BYTES + 1]),
        Err(AcceptanceError::Envelope)
    );
    assert!(
        serde_json::from_value::<ApplyProbe>(
            json!({"probe_id":"probe-01","expected_revision":-1,"value":0})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ProbeApplied>(
            json!({"probe_id":"probe-01","revision":0,"value":0})
        )
        .is_err()
    );
    // JSON Schema's integer semantics include integral floating representations;
    // Rust conversion remains a separate fail-closed step, not implicit rounding.
    let floating = json!({"probe_id":"probe-01","expected_revision":0,"value":42.0});
    SchemaKind::Command.validate(&floating)?;
    assert_eq!(
        AcceptanceMessage::from_json(&changed(COMMAND, "data", floating)?),
        Err(AcceptanceError::Payload)
    );
    fn send_sync<T: Send + Sync + 'static>() {}
    send_sync::<ApplyProbe>();
    send_sync::<ProbeApplied>();
    send_sync::<AcceptanceError>();
    Ok(())
}

#[test]
fn public_diagnostics_redact_payloads_and_rejected_metadata() -> TestResult {
    let payload = json!({"probe_id":"PRIVATE-sentinel-7391","expected_revision":0,"value":42});
    let error = AcceptanceMessage::from_json(&changed(COMMAND, "data", payload.clone())?)
        .err()
        .ok_or("unexpected success")?;
    for rendered in [
        format!("{error}"),
        format!("{error:?}"),
        format!("{:?}", serde_json::from_value::<ApplyProbe>(payload).err()),
    ] {
        assert!(!rendered.contains("PRIVATE-sentinel-7391"));
        assert!(!rendered.contains("[80, 82, 73"));
    }
    let command = ApplyProbe::new("private-sentinel-7391", 0, 42)?;
    let fact = ProbeApplied::new("private-sentinel-7391", 1, 42)?;
    for rendered in [
        format!("{command:?}"),
        format!("{fact:?}"),
        format!("{:?}", AcceptanceMessage::Command(command)),
    ] {
        assert!(!rendered.contains("private-sentinel-7391"));
    }
    assert!(std::error::Error::source(&error).is_none());
    let rejected_uri = AcceptanceMessage::from_json(&changed(
        COMMAND,
        "dataschema",
        json!("urn:private-schema-sentinel-7391"),
    )?)
    .err()
    .ok_or("unexpected route success")?;
    assert_eq!(rejected_uri, AcceptanceError::Routing);
    assert!(!format!("{rejected_uri:?} {rejected_uri}").contains("private-schema-sentinel-7391"));
    Ok(())
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "edgeagent-schema-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Exclusive creation means cleanup can only own this fixture's directory.
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
