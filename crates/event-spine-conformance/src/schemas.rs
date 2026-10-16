//! Deterministic local schema generation and conservative compatibility policy.

mod artifacts;
mod compatibility;

use std::sync::LazyLock;

use edgeagent_contracts::MessageDefinition;
use jsonschema::Validator;
use schemars::{JsonSchema, generate::SchemaSettings};
use serde_json::Value;

use crate::{
    AcceptanceError,
    contracts::{CommandPayload, FactPayload},
    definitions,
};

pub use artifacts::verify_artifacts;
pub use compatibility::verify_optional_additions;

static COMMAND: LazyLock<Result<Validator, AcceptanceError>> =
    LazyLock::new(|| compile(SchemaKind::Command));
static FACT: LazyLock<Result<Validator, AcceptanceError>> =
    LazyLock::new(|| compile(SchemaKind::Fact));

/// Exact local payload schema; this enum never accepts an untrusted URI/path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaKind {
    /// `ApplyProbe` v1 synthetic intent.
    Command,
    /// `ProbeApplied` v1 synthetic completed fact.
    Fact,
}

impl SchemaKind {
    /// The complete finite set, used by generation and artifact discovery.
    pub const ALL: [Self; 2] = [Self::Command, Self::Fact];

    /// Return the exact registry definition for this schema.
    #[must_use]
    pub const fn definition(self) -> MessageDefinition {
        match self {
            Self::Command => definitions()[0],
            Self::Fact => definitions()[1],
        }
    }

    /// Return the fixed filename inside `schemas/v1/`.
    #[must_use]
    pub const fn filename(self) -> &'static str {
        match self {
            Self::Command => "apply-probe.json",
            Self::Fact => "probe-applied.json",
        }
    }

    /// Generate a Draft 2020-12 schema from the private Serde wire type.
    ///
    /// # Errors
    /// Returns a bounded definition error if schema serialization fails.
    pub fn generate(self) -> Result<Value, AcceptanceError> {
        let mut schema = match self {
            Self::Command => generate::<CommandPayload>()?,
            Self::Fact => generate::<FactPayload>()?,
        };
        schema
            .as_object_mut()
            .ok_or(AcceptanceError::Definition)?
            .insert("$id".into(), Value::from(self.definition().data_schema()));
        Ok(schema)
    }

    /// Validate an already bounded JSON value against its local compiled schema.
    ///
    /// Direct callers must bound source JSON before parsing; raw envelope callers
    /// should use `AcceptanceMessage::from_json` for the 256 KiB input boundary.
    ///
    /// # Errors
    /// Returns `Payload` without validator instance text, or `Definition` for a
    /// faulty compiled-in schema. No filesystem or HTTP resolver is enabled.
    pub fn validate(self, value: &Value) -> Result<(), AcceptanceError> {
        let validator = match self {
            Self::Command => &*COMMAND,
            Self::Fact => &*FACT,
        };
        match validator {
            Ok(validator) if validator.is_valid(value) => Ok(()),
            Ok(_) => Err(AcceptanceError::Payload),
            Err(error) => Err(*error),
        }
    }

    /// Generate deterministic, recursively sorted pretty JSON with a final LF.
    ///
    /// # Errors
    /// Returns `Definition` if schema generation or serialization fails.
    pub fn artifact(self) -> Result<String, AcceptanceError> {
        canonical_json(self.generate()?)
    }
}

fn generate<T: JsonSchema>() -> Result<Value, AcceptanceError> {
    let settings =
        SchemaSettings::draft2020_12().with(|settings| settings.inline_subschemas = true);
    serde_json::to_value(settings.into_generator().into_root_schema_for::<T>())
        .map_err(|_| AcceptanceError::Definition)
}

fn compile(kind: SchemaKind) -> Result<Validator, AcceptanceError> {
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&kind.generate()?)
        .map_err(|_| AcceptanceError::Definition)
}

/// Serialize contract artifacts independently of serde_json feature unification.
///
/// # Errors
/// Returns a bounded definition error if JSON serialization fails.
pub fn canonical_json(mut value: Value) -> Result<String, AcceptanceError> {
    value.sort_all_objects();
    let mut text = serde_json::to_string_pretty(&value).map_err(|_| AcceptanceError::Definition)?;
    text.push('\n');
    Ok(text)
}
