use std::collections::{BTreeMap, BTreeSet};

use crate::repository::markdown::{Document, MarkdownTableRow};

use super::{ADR_DIRECTORY, ADR_INDEX, Record, Repository, Violation, adr_violation};

#[derive(Clone, Debug, Eq, PartialEq)]
struct IndexEntry {
    number: usize,
    target: String,
    status: String,
    line: usize,
}

pub(super) fn validate(
    repository: &Repository,
    records: &BTreeMap<usize, Record>,
) -> Vec<Violation> {
    let Some(document) = repository.document(ADR_INDEX) else {
        return vec![adr_violation(
            ADR_INDEX,
            1,
            "adr.index",
            "decision index is missing",
            "restore the index and register every decision exactly once",
        )];
    };
    let (entries, mut violations) = read_entries(document);
    let mut indexed: BTreeMap<usize, Vec<&IndexEntry>> = BTreeMap::new();
    for entry in &entries {
        indexed.entry(entry.number).or_default().push(entry);
    }

    for duplicate in indexed.values().filter(|items| items.len() > 1) {
        if let Some(entry) = duplicate.get(1) {
            violations.push(adr_violation(
                ADR_INDEX,
                entry.line,
                "adr.index",
                format!(
                    "ADR-{:04} appears more than once in the index",
                    entry.number
                ),
                "keep exactly one index row for every published decision",
            ));
        }
    }

    for pair in entries.windows(2) {
        if pair[1].number <= pair[0].number {
            violations.push(adr_violation(
                ADR_INDEX,
                pair[1].line,
                "adr.index",
                "decision index rows are not in ascending number order",
                "order rows by their four-digit ADR number",
            ));
        }
    }

    for (number, record) in records {
        match indexed.get(number).map(Vec::as_slice) {
            None | Some([]) => violations.push(adr_violation(
                &record.path,
                1,
                "adr.index",
                format!("ADR-{number:04} is not covered by the decision index"),
                "add exactly one index row linked to the decision",
            )),
            Some([entry]) => validate_entry(entry, record, &mut violations),
            Some(_) => {}
        }
    }
    for entry in entries {
        if !records.contains_key(&entry.number) {
            violations.push(adr_violation(
                ADR_INDEX,
                entry.line,
                "adr.index",
                format!(
                    "index covers ADR-{:04}, but no such decision exists",
                    entry.number
                ),
                "restore the historical decision or remove an unpublished stale row",
            ));
        }
    }
    violations
}

fn read_entries(document: &Document) -> (Vec<IndexEntry>, Vec<Violation>) {
    let headings: Vec<_> = document
        .headings()
        .iter()
        .enumerate()
        .filter(|(_, heading)| heading.level == 2 && heading.text.trim() == "Index")
        .collect();
    if headings.len() != 1 {
        return (
            Vec::new(),
            vec![adr_violation(
                ADR_INDEX,
                headings.first().map_or(1, |(_, heading)| heading.line),
                "adr.index",
                "decision index must contain exactly one rendered `Index` section",
                "restore one `## Index` heading around the canonical decision table",
            )],
        );
    }
    let (heading_index, heading) = headings[0];
    let end = document.headings()[heading_index + 1..]
        .iter()
        .find(|candidate| candidate.level <= 2)
        .map_or_else(
            || document.body().lines().count() + 1,
            |candidate| candidate.line,
        );
    let mut violations = Vec::new();
    if let Some(line) = document.first_raw_html_between(heading.line, end) {
        violations.push(adr_violation(
            ADR_INDEX,
            line,
            "adr.index",
            "decision index uses raw HTML around policy-bearing content",
            "express index links, statuses, and summaries with plain Markdown",
        ));
    }

    let source_lines: Vec<_> = document.body().lines().collect();
    let rows: Vec<_> = document
        .table_rows()
        .iter()
        .filter(|row| row.line > heading.line && row.line < end)
        .collect();
    let tables: BTreeSet<_> = rows.iter().map(|row| row.table).collect();
    if tables.len() != 1 {
        violations.push(adr_violation(
            ADR_INDEX,
            rows.get(1).map_or(heading.line, |row| row.line),
            "adr.index",
            "decision index must contain exactly one Markdown table",
            "keep the canonical header and every numbered decision row in one table",
        ));
    }
    let expected_header = ["ADR", "Status", "Decision"];
    let actual_header: Vec<_> = rows.first().map_or_else(Vec::new, |row| {
        row.cells.iter().map(|cell| cell.trim()).collect()
    });
    if actual_header != expected_header || rows.first().is_none_or(|row| !row.is_header) {
        violations.push(adr_violation(
            ADR_INDEX,
            rows.first().map_or(heading.line, |row| row.line),
            "adr.index",
            "decision index header differs from the canonical three-column contract",
            "restore the visible `ADR`, `Status`, and `Decision` header cells",
        ));
    }
    let mut entries = Vec::new();
    for row in rows.iter().skip(1) {
        if row.is_header {
            violations.push(adr_violation(
                ADR_INDEX,
                row.line,
                "adr.index",
                "decision index contains an additional rendered table header",
                "keep one table header followed only by numbered decision body rows",
            ));
            continue;
        }
        let Some(first_cell) = row.cells.first().map(|cell| cell.trim()) else {
            continue;
        };
        if first_cell == "ADR" {
            violations.push(adr_violation(
                ADR_INDEX,
                row.line,
                "adr.index",
                "decision index contains an additional header-like row",
                "keep one visible header followed only by numbered decision rows",
            ));
            continue;
        }
        match parse_entry(row, source_lines.get(row.line.saturating_sub(1)).copied()) {
            Ok(entry) => entries.push(entry),
            Err(message) => violations.push(adr_violation(
                ADR_INDEX,
                row.line,
                "adr.index",
                message,
                "use `| [ADR-NNNN](NNNN-lowercase-kebab-title.md) | Status | Decision |`",
            )),
        }
    }
    if entries.is_empty() {
        violations.push(adr_violation(
            ADR_INDEX,
            heading.line,
            "adr.index",
            "decision index contains no decision rows",
            "register every published decision in ascending number order",
        ));
    }
    (entries, violations)
}

fn parse_entry(row: &MarkdownTableRow, source: Option<&str>) -> Result<IndexEntry, String> {
    if row.cells.len() != 3 {
        return Err("decision index row must contain exactly three cells".to_owned());
    }
    let Some(label) = row.cells.first().map(|cell| cell.trim()) else {
        return Err("decision index row has no ADR label".to_owned());
    };
    let Some(number) = label.strip_prefix("ADR-") else {
        return Err(format!("index label `{label}` does not match `ADR-NNNN`"));
    };
    if number.len() != 4 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("index label `{label}` does not match `ADR-NNNN`"));
    }
    let number = number
        .parse::<usize>()
        .map_err(|error| format!("could not parse index label `{label}`: {error}"))?;
    if number == 0 {
        return Err("decision index cannot contain ADR-0000".to_owned());
    }
    let target = parse_target(source, label)?;
    let status = row.cells.get(1).map_or("", |cell| cell.trim());
    if !super::lifecycle::valid_status(status) {
        return Err(format!(
            "index status `{status}` is not a canonical lifecycle value"
        ));
    }
    let decision = row.cells.get(2).map_or("", |cell| cell.trim());
    if !decision.chars().any(char::is_alphanumeric) {
        return Err(format!("ADR-{number:04} has no visible decision summary"));
    }
    Ok(IndexEntry {
        number,
        target,
        status: status.to_owned(),
        line: row.line,
    })
}

fn parse_target(source: Option<&str>, expected_label: &str) -> Result<String, String> {
    let Some(source) = source else {
        return Err("could not read the decision index source row".to_owned());
    };
    let Some(first_cell) = source.trim().strip_prefix('|') else {
        return Err("decision index row is not a canonical Markdown table row".to_owned());
    };
    let Some((first_cell, _)) = first_cell.split_once('|') else {
        return Err("decision index row has no complete first cell".to_owned());
    };
    let first_cell = first_cell.trim();
    let Some(link) = first_cell.strip_prefix('[') else {
        return Err("decision index ADR label is not a Markdown link".to_owned());
    };
    let Some((label, destination)) = link.split_once("](") else {
        return Err("decision index ADR label is not a canonical Markdown link".to_owned());
    };
    let Some(destination) = destination.strip_suffix(')') else {
        return Err("decision index ADR link has a malformed destination".to_owned());
    };
    if label != expected_label {
        return Err(format!(
            "rendered index label `{expected_label}` differs from source label `{label}`"
        ));
    }
    Ok(destination.to_owned())
}

fn validate_entry(entry: &IndexEntry, record: &Record, violations: &mut Vec<Violation>) {
    let expected_target = record.path.trim_start_matches(ADR_DIRECTORY);
    if entry.target != expected_target {
        violations.push(adr_violation(
            ADR_INDEX,
            entry.line,
            "adr.index",
            format!(
                "ADR-{:04} links to `{}` instead of `{expected_target}`",
                entry.number, entry.target
            ),
            "link each index row to its exact decision filename",
        ));
    }
    if record.status.as_deref() != Some(entry.status.as_str()) {
        violations.push(adr_violation(
            ADR_INDEX,
            entry.line,
            "adr.index",
            format!(
                "ADR-{:04} index status `{}` differs from record status `{}`",
                entry.number,
                entry.status,
                record.status.as_deref().unwrap_or("invalid or missing")
            ),
            "make the index lifecycle status match the decision metadata",
        ));
    }
}
