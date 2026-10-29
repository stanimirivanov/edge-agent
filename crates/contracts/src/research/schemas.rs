//! Local structural schema; cross-field constraints require record validation.
use super::{ResearchContractError, ResearchRecord};
/// Draft 2020-12 schema with fixed v1 identity; never resolves remote references.
/// # Errors
/// Returns a payload-safe error if the compiled schema cannot be serialized.
pub fn generate() -> Result<String, ResearchContractError> {
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<ResearchRecord>();
    let mut value = serde_json::to_value(schema).map_err(|_| ResearchContractError)?;
    value["$id"] = "urn:edgeagent:schema:research:record:v1".into();
    let ordered =
        crate::messaging::canonical_json::to_vec(&value).map_err(|_| ResearchContractError)?;
    let value: serde_json::Value =
        serde_json::from_slice(&ordered).map_err(|_| ResearchContractError)?;
    let mut output = serde_json::to_string_pretty(&value).map_err(|_| ResearchContractError)?;
    output.push('\n');
    Ok(output)
}
