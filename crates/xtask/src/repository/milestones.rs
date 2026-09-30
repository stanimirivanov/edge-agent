use std::collections::{BTreeMap, BTreeSet};

use super::{MILESTONE_POLICY, Repository, Violation};

const ROADMAP: &str = "docs/roadmap/milestones.md";
const LATEST_PUBLISHED_MILESTONE: usize = 11;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Milestone {
    number: usize,
    title: String,
    outcome: String,
    line: usize,
}

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let Some(document) = repository.document(ROADMAP) else {
        return vec![milestone_violation(
            1,
            "milestone.roadmap",
            "milestone roadmap is missing",
            "restore the canonical roadmap with its index, milestone sections, and planning rules",
        )];
    };

    let (index, mut violations) = read_index(document);
    let (boundaries, boundary_violations) = read_boundaries(document);
    violations.extend(boundary_violations);
    violations.extend(validate_sequence(&index));
    violations.extend(validate_index_and_boundaries(&index, &boundaries));
    violations
}

fn read_index(
    document: &crate::repository::markdown::Document,
) -> (Vec<Milestone>, Vec<Violation>) {
    let mut milestones = Vec::new();
    let mut violations = Vec::new();
    let Some((start, end, duplicate)) = section_lines(document, "Milestone index") else {
        return (
            milestones,
            vec![milestone_violation(
                1,
                "milestone.index",
                "roadmap has no `Milestone index` section",
                "restore the canonical milestone table",
            )],
        );
    };
    if let Some(line) = duplicate {
        violations.push(milestone_violation(
            line,
            "milestone.index",
            "roadmap must contain exactly one rendered `Milestone index` section",
            "remove the duplicate section and keep one canonical milestone table",
        ));
    }
    if let Some(line) = document.first_raw_html_between(start, end) {
        violations.push(milestone_violation(
            line,
            "milestone.index",
            "milestone index uses raw HTML around policy-bearing content",
            "express milestone titles and outcomes with plain Markdown",
        ));
    }

    let rows: Vec<_> = document
        .table_rows()
        .iter()
        .filter(|row| row.line > start && row.line < end)
        .collect();
    let tables: BTreeSet<_> = rows.iter().map(|row| row.table).collect();
    if tables.len() != 1 {
        violations.push(milestone_violation(
            rows.get(1).map_or(start, |row| row.line),
            "milestone.index",
            "milestone index must contain exactly one Markdown table",
            "keep the canonical header and every milestone row in one table",
        ));
    }
    let has_canonical_header = rows.first().is_some_and(|row| {
        row.is_header
            && row.cells.len() == 2
            && row.cells.first().map(|cell| cell.trim()) == Some("Milestone")
            && row.cells.get(1).map(|cell| cell.trim()) == Some("Outcome")
    });
    if !has_canonical_header {
        violations.push(milestone_violation(
            rows.first().map_or(start, |row| row.line),
            "milestone.index",
            "milestone index must begin with the canonical `Milestone | Outcome` header row",
            "restore the two-column canonical header as the first row of the milestone table",
        ));
    }

    for (position, row) in rows.iter().enumerate() {
        let first = row.cells.first().map_or("", |cell| cell.trim());
        if position == 0 && has_canonical_header {
            continue;
        }
        if row.is_header {
            violations.push(milestone_violation(
                row.line,
                "milestone.index",
                "milestone index contains an additional rendered table header",
                "keep one table header followed only by numbered milestone body rows",
            ));
            continue;
        }
        if first == "Milestone" {
            violations.push(milestone_violation(
                row.line,
                "milestone.index",
                "milestone index header may appear only as the first table row",
                "remove the later header-like row and keep the canonical header first",
            ));
            continue;
        }

        if row.cells.len() != 2 {
            violations.push(milestone_violation(
                row.line,
                "milestone.index",
                "milestone index row must contain exactly a milestone and its outcome",
                "use one `MNN - Outcome` cell and one non-empty outcome cell",
            ));
            continue;
        }
        let Some(mut milestone) = parse_title(first, row.line) else {
            violations.push(milestone_violation(
                row.line,
                "milestone.title",
                format!("index value `{first}` does not match `MNN - Outcome`"),
                "use a two-digit positive number, one space on each side of the hyphen, and a non-empty title",
            ));
            continue;
        };
        let outcome = row.cells.get(1).map_or("", |cell| cell.trim());
        if !outcome.chars().any(char::is_alphanumeric) {
            violations.push(milestone_violation(
                row.line,
                "milestone.index",
                format!("{} has no observable outcome", milestone.title),
                "add a concise sentence describing what becomes true when the milestone completes",
            ));
        }
        milestone.outcome = outcome.to_owned();
        milestones.push(milestone);
    }

    if milestones.is_empty() {
        violations.push(milestone_violation(
            start,
            "milestone.index",
            "milestone index contains no milestone rows",
            "add at least `M01 - Outcome` and its observable outcome",
        ));
    }
    (milestones, violations)
}

fn read_boundaries(
    document: &crate::repository::markdown::Document,
) -> (Vec<Milestone>, Vec<Violation>) {
    let mut milestones = Vec::new();
    let mut violations = Vec::new();
    let headings = document.headings();

    for (index, heading) in headings.iter().enumerate() {
        if heading.level != 2 {
            continue;
        }
        let title = heading.text.trim();
        let Some(mut milestone) = parse_title(title, heading.line) else {
            if looks_like_milestone(title) {
                violations.push(milestone_violation(
                    heading.line,
                    "milestone.title",
                    format!("boundary heading `{title}` does not match `MNN - Outcome`"),
                    "use a direct level-two heading with the exact indexed milestone title",
                ));
            }
            continue;
        };

        let end = headings[index + 1..]
            .iter()
            .find(|candidate| candidate.level <= 2)
            .map_or_else(
                || document.body().lines().count() + 1,
                |candidate| candidate.line,
            );
        if let Some(line) = document.first_raw_html_between(heading.line, heading.line + 1) {
            violations.push(milestone_violation(
                line,
                "milestone.boundary",
                format!("milestone boundary heading `{title}` uses raw HTML"),
                "express the milestone heading with plain Markdown",
            ));
        }

        let outcome = boundary_outcome(document, heading.line, end);
        if let Some((_, line)) = &outcome
            && document.first_raw_html_between(*line, *line + 1).is_some()
        {
            violations.push(milestone_violation(
                *line,
                "milestone.boundary",
                format!("milestone boundary `{title}` outcome uses raw HTML"),
                "express the canonical outcome statement with plain Markdown",
            ));
        }
        if outcome.is_none() {
            violations.push(milestone_violation(
                heading.line,
                "milestone.boundary",
                format!("milestone boundary `{title}` has no visible `Outcome:` statement"),
                "place `**Outcome:**` followed by the indexed outcome immediately below the heading",
            ));
        }
        milestone.outcome = outcome.map_or_else(String::new, |(value, _)| value);
        milestones.push(milestone);
    }

    if milestones.is_empty() {
        violations.push(milestone_violation(
            1,
            "milestone.boundary",
            "roadmap contains no direct level-two milestone boundary sections",
            "add one `## MNN - Outcome` section for every indexed milestone",
        ));
    }
    (milestones, violations)
}

fn boundary_outcome(
    document: &crate::repository::markdown::Document,
    start: usize,
    end: usize,
) -> Option<(String, usize)> {
    for line in start + 1..end {
        let Some(text) = document.visible_line_text(line) else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let outcome = text.strip_prefix("Outcome:")?.trim();
        return outcome
            .chars()
            .any(char::is_alphanumeric)
            .then(|| (outcome.to_owned(), line));
    }
    None
}

fn validate_sequence(milestones: &[Milestone]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, milestone) in milestones.iter().enumerate() {
        if !seen.insert(milestone.number) {
            violations.push(milestone_violation(
                milestone.line,
                "milestone.sequence",
                format!(
                    "M{:02} appears more than once in the index",
                    milestone.number
                ),
                "keep one row for every published milestone number",
            ));
        }
        let expected = index + 1;
        if milestone.number != expected {
            violations.push(milestone_violation(
                milestone.line,
                "milestone.sequence",
                format!(
                    "milestone sequence expected M{expected:02}, found M{:02}",
                    milestone.number
                ),
                "start at M01 and keep published milestone numbers contiguous and ordered",
            ));
        }
    }
    let latest = milestones.last().map_or(0, |milestone| milestone.number);
    if latest != LATEST_PUBLISHED_MILESTONE {
        violations.push(milestone_violation(
            milestones.last().map_or(1, |milestone| milestone.line),
            "milestone.sequence",
            format!(
                "latest milestone is M{latest:02}; repository policy records M{LATEST_PUBLISHED_MILESTONE:02}"
            ),
            "restore deleted history, or advance `LATEST_PUBLISHED_MILESTONE` only while publishing the next milestone",
        ));
    }
    violations
}

fn validate_index_and_boundaries(index: &[Milestone], boundaries: &[Milestone]) -> Vec<Violation> {
    let indexed: BTreeMap<_, _> = index
        .iter()
        .map(|milestone| (milestone.number, milestone))
        .collect();
    let mut boundary_counts: BTreeMap<usize, Vec<&Milestone>> = BTreeMap::new();
    for boundary in boundaries {
        boundary_counts
            .entry(boundary.number)
            .or_default()
            .push(boundary);
    }

    let mut violations = Vec::new();
    for milestone in index {
        match boundary_counts.get(&milestone.number).map(Vec::as_slice) {
            None | Some([]) => violations.push(milestone_violation(
                milestone.line,
                "milestone.boundary",
                format!(
                    "{} has no direct level-two boundary section",
                    milestone.title
                ),
                "add a level-two heading with the exact indexed title and outcome",
            )),
            Some([boundary]) => {
                if boundary.title != milestone.title {
                    violations.push(milestone_violation(
                        boundary.line,
                        "milestone.boundary",
                        format!(
                            "boundary title `{}` differs from index title `{}`",
                            boundary.title, milestone.title
                        ),
                        "make the boundary heading exactly match the published index title",
                    ));
                }
                if !boundary.outcome.is_empty() && boundary.outcome != milestone.outcome {
                    violations.push(milestone_violation(
                        boundary.line,
                        "milestone.boundary",
                        format!(
                            "boundary outcome `{}` differs from index outcome `{}`",
                            boundary.outcome, milestone.outcome
                        ),
                        "copy the indexed outcome after `**Outcome:**`",
                    ));
                }
            }
            Some(duplicates) => violations.push(milestone_violation(
                duplicates[1].line,
                "milestone.boundary",
                format!(
                    "M{:02} has more than one boundary section",
                    milestone.number
                ),
                "keep exactly one direct level-two boundary for each indexed milestone",
            )),
        }
    }
    for boundary in boundaries {
        if !indexed.contains_key(&boundary.number) {
            violations.push(milestone_violation(
                boundary.line,
                "milestone.boundary",
                format!("{} has no index row", boundary.title),
                "add the published milestone to the index or remove an unpublished stale section",
            ));
        }
    }

    let index_order: Vec<_> = index.iter().map(|milestone| milestone.number).collect();
    let boundary_order: Vec<_> = boundaries
        .iter()
        .map(|milestone| milestone.number)
        .collect();
    if index_order != boundary_order {
        violations.push(milestone_violation(
            boundaries.first().map_or(1, |milestone| milestone.line),
            "milestone.boundary",
            "milestone boundary order differs from the canonical index order",
            "order direct level-two milestone sections exactly as they appear in the index",
        ));
    }
    violations
}

fn parse_title(value: &str, line: usize) -> Option<Milestone> {
    let (identifier, title) = value.split_once(" - ")?;
    let number = identifier.strip_prefix('M')?;
    if number.len() != 2
        || !number.bytes().all(|byte| byte.is_ascii_digit())
        || !title.chars().any(char::is_alphanumeric)
        || title != title.trim()
    {
        return None;
    }
    let number = number.parse().ok()?;
    (number > 0).then(|| Milestone {
        number,
        title: value.to_owned(),
        outcome: String::new(),
        line,
    })
}

fn looks_like_milestone(value: &str) -> bool {
    value
        .strip_prefix('M')
        .and_then(|remainder| remainder.bytes().next())
        .is_some_and(|byte| byte.is_ascii_digit())
}

fn section_lines(
    document: &crate::repository::markdown::Document,
    name: &str,
) -> Option<(usize, usize, Option<usize>)> {
    let headings = document.headings();
    let mut matching = headings
        .iter()
        .enumerate()
        .filter(|(_, heading)| heading.level == 2 && heading.text.trim() == name);
    let (index, heading) = matching.next()?;
    let duplicate = matching.next().map(|(_, heading)| heading.line);
    let end = headings[index + 1..]
        .iter()
        .find(|candidate| candidate.level <= 2)
        .map_or_else(
            || document.body().lines().count() + 1,
            |candidate| candidate.line,
        );
    Some((heading.line, end, duplicate))
}

fn milestone_violation(
    line: usize,
    rule: &'static str,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation::new(ROADMAP, line, rule, message, fix, MILESTONE_POLICY)
}

#[cfg(test)]
mod tests;
