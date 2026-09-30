use std::collections::{BTreeMap, BTreeSet};

use super::{Record, Violation, adr_violation};

pub(super) fn parse_references(value: &str, allow_multiple: bool) -> Result<Vec<usize>, String> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let mut references = Vec::new();
    let mut seen = BTreeSet::new();
    for item in value.split(',') {
        let reference = item.trim();
        let Some(number) = reference.strip_prefix("ADR-") else {
            return Err(format!(
                "supersession value `{value}` contains a non-canonical reference"
            ));
        };
        if number.len() != 4 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!(
                "supersession value `{value}` contains a non-canonical reference"
            ));
        }
        let Ok(number) = number.parse::<usize>() else {
            return Err(format!(
                "supersession value `{value}` contains an invalid ADR number"
            ));
        };
        if number == 0 {
            return Err("supersession references must use a positive ADR number".to_owned());
        }
        if !seen.insert(number) {
            return Err(format!(
                "supersession value `{value}` repeats ADR-{number:04}"
            ));
        }
        references.push(number);
    }
    if !allow_multiple && references.len() > 1 {
        return Err("`Superseded by` names more than one replacement decision".to_owned());
    }
    Ok(references)
}

pub(super) fn validate(records: &BTreeMap<usize, Record>) -> Vec<Violation> {
    let mut violations = Vec::new();
    for record in records.values() {
        validate_lifecycle(record, &mut violations);
        for superseded in &record.supersedes {
            if *superseded >= record.number {
                violations.push(adr_violation(
                    &record.path,
                    record.supersedes_line,
                    "adr.supersession",
                    format!(
                        "ADR-{:04} cannot supersede itself or later ADR-{superseded:04}",
                        record.number
                    ),
                    "list only earlier decisions in `Supersedes` metadata",
                ));
            }
            let Some(target) = records.get(superseded) else {
                violations.push(missing_reference(
                    record,
                    *superseded,
                    record.supersedes_line,
                ));
                continue;
            };
            if target.superseded_by != Some(record.number) {
                violations.push(adr_violation(
                    &target.path,
                    target.superseded_by_line,
                    "adr.supersession",
                    format!(
                        "ADR-{:04} supersedes ADR-{superseded:04}, but the earlier decision does not record the reciprocal replacement",
                        record.number
                    ),
                    "record both sides of the supersession relationship",
                ));
            }
        }

        if let Some(replacement) = record.superseded_by {
            if replacement <= record.number {
                violations.push(adr_violation(
                    &record.path,
                    record.superseded_by_line,
                    "adr.supersession",
                    format!(
                        "ADR-{:04} names itself or earlier ADR-{replacement:04} as its replacement",
                        record.number
                    ),
                    "name exactly one later decision in `Superseded by` metadata",
                ));
            }
            let Some(target) = records.get(&replacement) else {
                violations.push(missing_reference(
                    record,
                    replacement,
                    record.superseded_by_line,
                ));
                continue;
            };
            if !target.supersedes.contains(&record.number) {
                violations.push(adr_violation(
                    &target.path,
                    target.supersedes_line,
                    "adr.supersession",
                    format!(
                        "ADR-{replacement:04} replaces ADR-{:04}, but does not record the reciprocal `Supersedes` reference",
                        record.number
                    ),
                    "record both sides of the supersession relationship",
                ));
            }
        }
    }
    violations
}

fn validate_lifecycle(record: &Record, violations: &mut Vec<Violation>) {
    let is_superseded = record.status.as_deref() == Some("Superseded");
    if !record.supersedes.is_empty()
        && !matches!(record.status.as_deref(), Some("Accepted" | "Superseded"))
    {
        violations.push(adr_violation(
            &record.path,
            record.status_line,
            "adr.supersession",
            "only an Accepted or already-Superseded decision may supersede earlier decisions",
            "clear `Supersedes` while the replacement is Proposed, Rejected, or Deprecated; establish reciprocal supersession when it is Accepted",
        ));
    }
    if is_superseded && record.superseded_by.is_none() {
        violations.push(adr_violation(
            &record.path,
            record.status_line,
            "adr.supersession",
            "Superseded lifecycle status does not name a replacement decision",
            "name the later replacement in `Superseded by` metadata",
        ));
    } else if !is_superseded && record.superseded_by.is_some() {
        violations.push(adr_violation(
            &record.path,
            record.status_line,
            "adr.supersession",
            "decision names a replacement but its lifecycle status is not Superseded",
            "set the status to Superseded or clear the replacement metadata",
        ));
    }
}

fn missing_reference(record: &Record, reference: usize, line: usize) -> Violation {
    adr_violation(
        &record.path,
        line,
        "adr.supersession",
        format!("supersession metadata references missing ADR-{reference:04}"),
        "reference an existing indexed decision",
    )
}
