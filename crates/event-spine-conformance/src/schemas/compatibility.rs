use serde_json::Value;

use crate::AcceptanceError;

/// Accept only identical schemas or new optional top-level object properties.
///
/// Every existing property's complete schema and every other root keyword must
/// remain identical. Both schemas must allow unknown properties. This is a
/// conservative candidate-policy check, not JSON Schema subsumption, semantic
/// compatibility, or permission to overwrite a published immutable schema URI.
///
/// # Errors
/// Returns `Compatibility` for unsupported shape, changed constraints/identity,
/// removed properties, required additions, or any unclassified change.
pub fn verify_optional_additions(
    baseline: &Value,
    candidate: &Value,
) -> Result<(), AcceptanceError> {
    let fail = AcceptanceError::Compatibility;
    let old = baseline.as_object().ok_or(fail)?;
    let new = candidate.as_object().ok_or(fail)?;
    for schema in [old, new] {
        if schema.get("type") != Some(&Value::from("object"))
            || schema.get("$schema")
                != Some(&Value::from("https://json-schema.org/draft/2020-12/schema"))
            || !matches!(
                schema.get("additionalProperties"),
                None | Some(Value::Bool(true))
            )
        {
            return Err(fail);
        }
    }
    // Exact root-key equality rejects newly introduced dependentRequired,
    // unevaluatedProperties, allOf, refs, or other potentially narrowing rules.
    if old.len() != new.len()
        || old
            .iter()
            .any(|(key, value)| key != "properties" && new.get(key) != Some(value))
    {
        return Err(fail);
    }
    let old_properties = old
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(fail)?;
    let new_properties = new
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(fail)?;
    if old_properties
        .iter()
        .any(|(name, schema)| new_properties.get(name) != Some(schema))
    {
        return Err(fail);
    }
    let required = old.get("required").and_then(Value::as_array).ok_or(fail)?;
    if required.iter().any(|name| {
        name.as_str()
            .is_none_or(|name| !old_properties.contains_key(name))
    }) {
        return Err(fail);
    }
    Ok(())
}
