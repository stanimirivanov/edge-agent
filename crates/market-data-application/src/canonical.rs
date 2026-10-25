//! Shared owner-private canonical JSON; existing publication bytes stay unchanged.
use crate::{BudgetError as Error, BudgetErrorKind as K};

pub(crate) fn canonical(value: serde_json::Value) -> Result<Vec<u8>, Error> {
    fn sorted(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(fields) => {
                let fields = fields
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>();
                serde_json::Value::Object(fields.into_iter().map(|(k, v)| (k, sorted(v))).collect())
            }
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(sorted).collect())
            }
            other => other,
        }
    }
    serde_json::to_vec(&sorted(value)).map_err(|_| Error::new(K::Invalid))
}
