//! Synthetic service policy; transport identity and broker order are irrelevant.

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Decision {
    Apply,
    Duplicate,
    Conflict,
    MissingPredecessor,
    Stale,
}

pub(super) fn decide(current: i64, expected: u32, value: u32, receipt: Option<i64>) -> Decision {
    // Compare accepted content before current revision: old identical work stays
    // idempotent even after the aggregate advances through subsequent commands.
    if let Some(accepted) = receipt {
        return if accepted == i64::from(value) {
            Decision::Duplicate
        } else {
            Decision::Conflict
        };
    }
    match current.cmp(&i64::from(expected)) {
        std::cmp::Ordering::Equal => Decision::Apply,
        std::cmp::Ordering::Less => Decision::MissingPredecessor,
        std::cmp::Ordering::Greater => Decision::Stale,
    }
}

#[cfg(test)]
mod tests;
