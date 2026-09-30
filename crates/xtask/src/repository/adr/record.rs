use std::collections::{BTreeMap, BTreeSet};

use crate::repository::markdown::Document;

use super::{
    ADR_DIRECTORY, METADATA_FIELDS, PUBLISHED_ADRS, REQUIRED_SECTIONS, Record, Repository,
    Violation, adr_violation, lifecycle, supersession,
};

const ROADMAP: &str = "docs/roadmap/milestones.md";

pub(super) fn read(repository: &Repository) -> (BTreeMap<usize, Record>, Vec<Violation>) {
    let mut records = BTreeMap::new();
    let mut violations = Vec::new();

    for (path, document) in &repository.documents {
        let Some(file_name) = path.strip_prefix(ADR_DIRECTORY) else {
            continue;
        };
        if file_name == "README.md" {
            continue;
        }
        if file_name.contains('/') {
            violations.push(adr_violation(
                path,
                1,
                "adr.location",
                "decision Markdown is nested below the canonical decision directory",
                "move every decision record directly under `docs/decisions` and register it in the decision index",
            ));
            continue;
        }
        let Some(number) = parse_file_name(file_name) else {
            violations.push(adr_violation(
                path,
                1,
                "adr.filename",
                "ADR filename does not use a four-digit number and lowercase kebab-case title",
                "rename it to `NNNN-short-decision-title.md` and update the decision index and inbound links",
            ));
            continue;
        };

        let mut record = Record::new(number, path.clone());
        violations.extend(validate_record(document, &mut record));
        violations.extend(validate_published_identity(document, &record));
        if let Some(previous) = records.insert(number, record) {
            violations.push(adr_violation(
                path,
                1,
                "adr.number",
                format!("ADR number duplicates `{}`", previous.path),
                "renumber the unpublished decision to the lowest unused number and update its references",
            ));
        }
    }

    (records, violations)
}

fn validate_published_identity(document: &Document, record: &Record) -> Vec<Violation> {
    let Some(identity) = PUBLISHED_ADRS
        .iter()
        .find(|identity| identity.number == record.number)
    else {
        return Vec::new();
    };
    let mut violations = Vec::new();
    if record.path != identity.path {
        violations.push(adr_violation(
            &record.path,
            1,
            "adr.identity",
            format!(
                "published ADR-{:04} path differs from canonical history `{}`",
                record.number, identity.path
            ),
            "restore the published filename; create a new ADR to replace or supersede its decision",
        ));
    }

    let expected_heading = format!("ADR-{:04}: {}", identity.number, identity.title);
    let heading = document.headings().first();
    if !heading.is_some_and(|heading| {
        heading.level == 1 && heading.line == 1 && heading.text == expected_heading
    }) {
        violations.push(adr_violation(
            &record.path,
            heading.map_or(1, |heading| heading.line),
            "adr.identity",
            format!(
                "published ADR-{:04} title differs from canonical history `{}`",
                record.number, identity.title
            ),
            "restore the published title; create a new ADR to replace or supersede its decision",
        ));
    }
    violations
}

pub(super) fn validate_record(document: &Document, record: &mut Record) -> Vec<Violation> {
    let mut violations = Vec::new();
    if !has_canonical_heading(document, record.number) {
        violations.push(adr_violation(
            &record.path,
            1,
            "adr.heading",
            format!(
                "first heading does not match `# ADR-{:04}: Decision title`",
                record.number
            ),
            "make the first line use the filename's ADR number and a non-empty title",
        ));
    }

    violations.extend(validate_exact_sections(document, &record.path));
    violations.extend(validate_metadata(document, record));
    violations
}

pub(super) fn validate_milestones(
    repository: &Repository,
    records: &BTreeMap<usize, Record>,
) -> Vec<Violation> {
    let Some(roadmap) = repository.document(ROADMAP) else {
        return Vec::new();
    };
    let published = published_milestones(roadmap);
    records
        .values()
        .filter_map(|record| {
            let milestone = record.milestone.as_deref()?;
            let number = lifecycle::milestone_number(milestone)?;
            match published.get(&number) {
                Some(expected) if expected == milestone => None,
                Some(expected) => Some(adr_violation(
                    &record.path,
                    record.milestone_line,
                    "adr.milestone",
                    format!("milestone `{milestone}` differs from published title `{expected}`"),
                    "copy the exact milestone title from the roadmap index",
                )),
                None => Some(adr_violation(
                    &record.path,
                    record.milestone_line,
                    "adr.milestone",
                    format!("milestone `{milestone}` is not published in the roadmap index"),
                    "use an exact published milestone title from the roadmap index",
                )),
            }
        })
        .collect()
}

pub(super) fn parse_file_name(file_name: &str) -> Option<usize> {
    let stem = file_name.strip_suffix(".md")?;
    let (number, slug) = stem.split_once('-')?;
    if number.len() != 4 || !number.bytes().all(|byte| byte.is_ascii_digit()) || !valid_slug(slug) {
        return None;
    }
    let number = number.parse().ok()?;
    (number > 0).then_some(number)
}

pub(super) fn has_canonical_heading(document: &Document, number: usize) -> bool {
    let expected_prefix = format!("ADR-{number:04}: ");
    document.headings().first().is_some_and(|heading| {
        heading.level == 1
            && heading.line == 1
            && heading.text.starts_with(&expected_prefix)
            && heading.text[expected_prefix.len()..]
                .chars()
                .any(char::is_alphanumeric)
            && document
                .first_raw_html_between(heading.line, heading.line + 1)
                .is_none()
    })
}

pub(super) fn validate_exact_sections(document: &Document, path: &str) -> Vec<Violation> {
    let sections: Vec<_> = document
        .headings()
        .iter()
        .filter(|heading| heading.level == 2)
        .map(|heading| (heading.text.trim().to_owned(), heading.line))
        .collect();
    let actual: Vec<_> = sections.iter().map(|(name, _)| name.as_str()).collect();
    let mut violations = Vec::new();
    if actual != REQUIRED_SECTIONS {
        violations.push(adr_violation(
            path,
            sections.first().map_or(1, |(_, line)| *line),
            "adr.sections",
            format!(
                "ADR sections `{}` do not match the canonical order `{}`",
                actual.join(", "),
                REQUIRED_SECTIONS.join(", ")
            ),
            "restore each canonical level-two section exactly once and in the documented order",
        ));
    }
    for (_, line) in sections {
        if document.first_raw_html_between(line, line + 1).is_some() {
            violations.push(adr_violation(
                path,
                line,
                "adr.sections",
                "required ADR heading uses raw HTML",
                "write required decision headings with plain Markdown",
            ));
        }
    }
    violations
}

fn validate_metadata(document: &Document, record: &mut Record) -> Vec<Violation> {
    let first_section_line = document
        .headings()
        .iter()
        .find(|heading| heading.level == 2)
        .map_or_else(
            || document.body().lines().count() + 1,
            |heading| heading.line,
        );
    let title_line = document
        .headings()
        .iter()
        .find(|heading| heading.level == 1)
        .map_or(0, |heading| heading.line);
    let metadata = metadata_lines(document, title_line, first_section_line);
    let mut violations = Vec::new();

    if let Some(line) = document.first_raw_html_between(title_line + 1, first_section_line) {
        violations.push(adr_violation(
            &record.path,
            line,
            "adr.metadata",
            "ADR lifecycle metadata uses raw HTML",
            "express every metadata field with plain Markdown list items",
        ));
    }

    let actual_fields: Vec<_> = metadata.iter().map(|item| item.field.as_str()).collect();
    if actual_fields != METADATA_FIELDS {
        violations.push(adr_violation(
            &record.path,
            metadata.first().map_or(title_line + 1, |item| item.line),
            "adr.metadata",
            format!(
                "ADR metadata fields `{}` do not match the canonical order `{}`",
                actual_fields.join(", "),
                METADATA_FIELDS.join(", ")
            ),
            "restore exactly one Status, Date, Milestone, Deciders, Supersedes, and Superseded by list item in that order",
        ));
    }

    let distinct: BTreeSet<_> = actual_fields.iter().copied().collect();
    if distinct.len() != actual_fields.len() {
        violations.push(adr_violation(
            &record.path,
            metadata.first().map_or(title_line + 1, |item| item.line),
            "adr.metadata",
            "ADR metadata contains a duplicate field",
            "keep exactly one list item for every canonical metadata field",
        ));
    }

    for item in metadata {
        match item.field.as_str() {
            "Status" => {
                record.status_line = item.line;
                if lifecycle::valid_status(&item.value) {
                    record.status = Some(item.value);
                } else {
                    violations.push(adr_violation(
                        &record.path,
                        item.line,
                        "adr.status",
                        "status is not one of Proposed, Accepted, Rejected, Deprecated, or Superseded",
                        "use one canonical case-sensitive lifecycle value",
                    ));
                }
            }
            "Date" if !lifecycle::valid_iso_date(&item.value) => {
                violations.push(adr_violation(
                    &record.path,
                    item.line,
                    "adr.date",
                    "decision date is not a valid ISO calendar date",
                    "use `- Date: YYYY-MM-DD` with a real calendar date",
                ));
            }
            "Milestone" => {
                record.milestone_line = item.line;
                if lifecycle::valid_milestone(&item.value) {
                    record.milestone = Some(item.value);
                } else {
                    violations.push(adr_violation(
                        &record.path,
                        item.line,
                        "adr.milestone",
                        "milestone does not match `MNN - Outcome`",
                        "use a positive two-digit milestone number and non-empty outcome",
                    ));
                }
            }
            "Deciders" if !item.value.chars().any(char::is_alphanumeric) => {
                violations.push(adr_violation(
                    &record.path,
                    item.line,
                    "adr.deciders",
                    "deciders metadata is empty",
                    "name the maintainers or decision owners",
                ));
            }
            "Supersedes" => {
                record.supersedes_line = item.line;
                match supersession::parse_references(&item.value, true) {
                    Ok(references) => record.supersedes = references,
                    Err(message) => violations.push(adr_violation(
                        &record.path,
                        item.line,
                        "adr.supersession",
                        message,
                        "leave the value empty or use a comma-separated list of `ADR-NNNN` references",
                    )),
                }
            }
            "Superseded by" => {
                record.superseded_by_line = item.line;
                match supersession::parse_references(&item.value, false) {
                    Ok(references) => record.superseded_by = references.first().copied(),
                    Err(message) => violations.push(adr_violation(
                        &record.path,
                        item.line,
                        "adr.supersession",
                        message,
                        "leave the value empty or name exactly one later decision as `ADR-NNNN`",
                    )),
                }
            }
            _ => {}
        }
    }
    violations
}

fn published_milestones(document: &Document) -> BTreeMap<usize, String> {
    let Some((heading_index, heading)) = document
        .headings()
        .iter()
        .enumerate()
        .find(|(_, heading)| heading.level == 2 && heading.text.trim() == "Milestone index")
    else {
        return BTreeMap::new();
    };
    let end = document.headings()[heading_index + 1..]
        .iter()
        .find(|candidate| candidate.level <= 2)
        .map_or_else(
            || document.body().lines().count() + 1,
            |candidate| candidate.line,
        );
    document
        .table_rows()
        .iter()
        .filter(|row| row.line > heading.line && row.line < end)
        .filter_map(|row| row.cells.first().map(|cell| cell.trim()))
        .filter_map(|title| lifecycle::milestone_number(title).map(|number| (number, title)))
        .map(|(number, title)| (number, title.to_owned()))
        .collect()
}

fn metadata_lines(document: &Document, title_line: usize, end: usize) -> Vec<MetadataLine> {
    document
        .body()
        .lines()
        .enumerate()
        .filter_map(|(index, source)| {
            let line = index + 1;
            if line <= title_line
                || line >= end
                || source.trim().is_empty()
                || !document.line_is_visible(line)
            {
                return None;
            }
            let source = source.trim();
            let parsed = source
                .strip_prefix("- ")
                .and_then(|value| value.split_once(':'));
            Some(parsed.map_or_else(
                || MetadataLine {
                    field: format!("invalid `{source}`"),
                    value: String::new(),
                    line,
                },
                |(field, value)| MetadataLine {
                    field: field.trim().to_owned(),
                    value: value.trim().to_owned(),
                    line,
                },
            ))
        })
        .collect()
}

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

struct MetadataLine {
    field: String,
    value: String,
    line: usize,
}
