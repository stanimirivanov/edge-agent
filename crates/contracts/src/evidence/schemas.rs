//! Optional tooling-only schema generation; no runtime URI resolution.
use super::*;
use serde_json::Value;
/// Complete v1 local artifact/message schema census.
#[derive(Clone, Copy, Debug)]
pub enum EvidenceSchema {
    /// Independent date-only temporal value; not a daily-v1 object.
    DateOnlyTime,
    /// Canonical daily object.
    Artifact,
    /// Identity-only command.
    Acquire,
    /// Terminal ready/rejected decision.
    Decision,
}
impl EvidenceSchema {
    /// Fixed current schema inventory.
    pub const ALL: [Self; 4] = [
        Self::Artifact,
        Self::Acquire,
        Self::Decision,
        Self::DateOnlyTime,
    ];
    /// Exact immutable schema identifier.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::DateOnlyTime => "urn:edgeagent:schema:market-data:date-only-time:v1",
            Self::Artifact => "urn:edgeagent:schema:market-data:evidence-artifact:v1",
            Self::Acquire => DEFINITIONS[0].data_schema(),
            Self::Decision => DEFINITIONS[1].data_schema(),
        }
    }
    /// Fixed filename, never an untrusted URI/path.
    #[must_use]
    pub const fn filename(self) -> &'static str {
        match self {
            Self::DateOnlyTime => "date-only-time-v1.json",
            Self::Artifact => "daily-v1.json",
            Self::Acquire => "acquire-v1.json",
            Self::Decision => "decided-v1.json",
        }
    }
    /// Generate deterministic local Draft 2020-12 structural schemas.
    /// Cross-field chronology/decimal/lineage invariants additionally require Rust validation.
    /// # Errors
    /// Rejects an invalid compiled-in schema serialization.
    pub fn generate(self) -> Result<Value, EvidenceContractError> {
        let settings =
            schemars::generate::SchemaSettings::draft2020_12().with(|s| s.inline_subschemas = true);
        let generator = settings.into_generator();
        let schema = match self {
            Self::DateOnlyTime => generator.into_root_schema_for::<DateOnlyTimeData>(),
            Self::Artifact => generator.into_root_schema_for::<DailyEvidence>(),
            Self::Acquire => generator.into_root_schema_for::<AcquireEvidence>(),
            Self::Decision => generator.into_root_schema_for::<EvidenceDecisionData>(),
        };
        let mut value = serde_json::to_value(schema).map_err(|_| EvidenceContractError)?;
        // Every v1 DTO field is required; explicit null is distinct from omission.
        fn require_fields(v: &mut Value) {
            match v {
                Value::Object(o) => {
                    if let Some(Value::Object(fields)) = o.get("properties") {
                        let mut keys: Vec<_> = fields.keys().cloned().collect();
                        keys.sort();
                        let required = keys.into_iter().map(Value::String).collect();
                        o.insert("required".into(), Value::Array(required));
                    }
                    for child in o.values_mut() {
                        require_fields(child);
                    }
                }
                Value::Array(a) => {
                    for child in a {
                        require_fields(child);
                    }
                }
                _ => {}
            }
        }
        require_fields(&mut value);
        value
            .as_object_mut()
            .ok_or(EvidenceContractError)?
            .insert("$id".into(), self.id().into());
        value.sort_all_objects();
        Ok(value)
    }
    /// Sorted pretty JSON with one final LF, for committed schema artifacts only.
    /// # Errors
    /// Rejects an invalid compiled-in schema serialization.
    pub fn artifact(self) -> Result<String, EvidenceContractError> {
        let mut text =
            serde_json::to_string_pretty(&self.generate()?).map_err(|_| EvidenceContractError)?;
        text.push('\n');
        Ok(text)
    }
}
