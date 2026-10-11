use std::fs;

use crate::verification::DEFAULT_COMMAND;

use super::{
    CONTRIBUTING, PULL_REQUEST_TEMPLATE, Repository, Violation, document_second_level_headings,
    first_second_level, heading_line, line_or_start, normalized_visible_section_text,
    pull_request_path_violation, pull_request_violation, section_lines,
    verification_policy_violation, visible_section_source_lines,
};

const TEMPLATE_LOCATIONS: &[&str] = &["", ".github", "docs"];
const TEMPLATE_BASENAME: &str = "PULL_REQUEST_TEMPLATE";

const PULL_REQUEST_HEADINGS: [&str; 8] = [
    "TL;DR",
    "Goal",
    "Scope and exclusions",
    "Design and compatibility",
    "Architecture",
    "Verification",
    "Risk and operations",
    "Review checklist",
];

const CANONICAL_SECTION_TEXT: [(&str, &str); 6] = [
    (
        "TL;DR",
        "State the one outcome delivered, the highest material risk, and the verification result.",
    ),
    (
        "Goal",
        "Describe the problem and observable result. Issue: Milestone:",
    ),
    ("Scope and exclusions", "Included: Deliberately excluded:"),
    (
        "Design and compatibility",
        "Describe material decisions, assumptions, contracts, event compatibility, service ownership, data meaning, deployment profiles, and ADRs. Unresolved questions: Known limitations: Follow-up work:",
    ),
    (
        "Architecture",
        "Owning capability and package role: Inbound caller or adapter: Consumed ports and outbound adapters: Internal dependency edges added, removed, or reclassified: End-to-end behavior proved:",
    ),
    (
        "Risk and operations",
        "Describe security, market-data integrity, model trust, event delivery/replay, dry-run execution boundaries, migration, rollout, rollback, deployment portability, and operational effects. Write “None” only after reviewing each area.",
    ),
];

const VERIFICATION_PROMPTS: [&str; 2] = ["**Checks not run and blocker:**", "**Residual risk:**"];

const VERIFICATION_HEADERS: [&str; 3] =
    ["Command or check", "Outcome", "Evidence or reason not run"];

const REQUIRED_VERIFICATION_COMMANDS: [&str; 3] = [
    "cargo xtask repository",
    "cargo xtask architecture",
    DEFAULT_COMMAND,
];

pub(super) const VERIFICATION_DOCUMENTS: [(&str, &str); 2] = [
    (CONTRIBUTING, "Verification and constrained environments"),
    ("README.md", "Verify the current foundation"),
];

pub(super) const PULL_REQUEST_CHECKLIST: [&str; 9] = [
    "- [ ] The change delivers one coherent capability.",
    "- [ ] Repository and architecture policy checks pass.",
    "- [ ] Package ownership and dependency changes are explicit and point inward.",
    "- [ ] Important success, rejection, and failure paths are tested.",
    "- [ ] Market facts and model output cross explicit validation boundaries.",
    "- [ ] Stateful event handling is idempotent under duplicates and redelivery.",
    "- [ ] Execution changes accept only dry-run operations and introduce no live broker path.",
    "- [ ] Public contracts and documentation changed with the implementation.",
    "- [ ] No secrets, licensed datasets, personal data, or generated local artifacts are committed.",
];

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let mut violations = inventory_violations(repository);
    for (path, heading) in VERIFICATION_DOCUMENTS {
        violations.extend(match repository.document(path) {
            Some(policy) => verification_section_violations(policy, path, heading),
            None => vec![verification_policy_violation(
                path,
                1,
                "canonical verification entry-point document is missing",
                "restore the document and its checked verification command block",
            )],
        });
    }
    let Some(template) = repository.document(PULL_REQUEST_TEMPLATE) else {
        violations.push(pull_request_violation(
            1,
            "pull request template is missing",
            "restore the checked-in review template",
        ));
        return violations;
    };
    violations.extend(validate_document(template));
    violations
}

fn inventory_violations(repository: &Repository) -> Vec<Violation> {
    let mut violations = Vec::new();
    for parent in TEMPLATE_LOCATIONS {
        let directory = repository.root().join(parent);
        let mut entries = match fs::read_dir(&directory) {
            Ok(entries) => match entries.collect::<Result<Vec<_>, _>>() {
                Ok(entries) => entries,
                Err(error) => {
                    violations.push(pull_request_path_violation(
                        display_parent(parent),
                        1,
                        format!("could not inventory GitHub pull request templates: {error}"),
                        "restore readable template locations containing only the canonical root template",
                    ));
                    continue;
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                violations.push(pull_request_path_violation(
                    display_parent(parent),
                    1,
                    format!("could not inventory GitHub pull request templates: {error}"),
                    "restore readable template locations containing only the canonical root template",
                ));
                continue;
            }
        };
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !has_template_basename(&name) {
                continue;
            }
            let path = entry_path(parent, &name);
            match entry.file_type() {
                Ok(file_type) if path == PULL_REQUEST_TEMPLATE && file_type.is_file() => {}
                Ok(_) => violations.push(pull_request_path_violation(
                    &path,
                    1,
                    format!("alternate GitHub pull request template entry `{path}` can bypass the governed template"),
                    "remove the alternate entry and retain only .github/PULL_REQUEST_TEMPLATE.md",
                )),
                Err(error) => violations.push(pull_request_path_violation(
                    &path,
                    1,
                    format!("could not inspect GitHub pull request template entry `{path}`: {error}"),
                    "restore the canonical regular template file or remove the alternate entry",
                )),
            }
        }
    }
    violations
}

fn has_template_basename(name: &str) -> bool {
    std::path::Path::new(name).file_stem().is_some_and(|stem| {
        stem.to_string_lossy()
            .eq_ignore_ascii_case(TEMPLATE_BASENAME)
    })
}

fn entry_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

fn display_parent(parent: &str) -> &str {
    if parent.is_empty() { "." } else { parent }
}

fn verification_section_violations(
    document: &crate::repository::markdown::Document,
    path: &str,
    heading: &str,
) -> Vec<Violation> {
    let heading_count = document
        .headings()
        .iter()
        .filter(|candidate| candidate.level == 2 && candidate.text.trim() == heading)
        .count();
    if heading_count != 1 {
        return vec![verification_policy_violation(
            path,
            1,
            format!(
                "verification document contains {heading_count} rendered `## {heading}` sections; expected one"
            ),
            "restore one canonical verification section and its fenced `text` command block",
        )];
    }
    let Some((start, end)) = section_lines(document, heading) else {
        return vec![verification_policy_violation(
            path,
            1,
            "canonical verification section could not be located",
            "restore the canonical rendered verification heading",
        )];
    };
    let blocks: Vec<_> = document
        .code_blocks()
        .iter()
        .filter(|block| block.line > start && block.line < end && block.info.trim() == "text")
        .collect();
    if blocks.len() != 1 {
        return vec![verification_policy_violation(
            path,
            start,
            format!(
                "verification section contains {} fenced `text` command blocks; expected one",
                blocks.len()
            ),
            "restore one fenced `text` block containing only the canonical verification runner command",
        )];
    }
    let Some(block) = blocks.first() else {
        return Vec::new();
    };
    let commands: Vec<_> = block.body.lines().collect();
    if commands == [DEFAULT_COMMAND] {
        return Vec::new();
    }
    vec![verification_policy_violation(
        path,
        block.line,
        "canonical verification command has drifted from the executable quality gate",
        format!("restore `{DEFAULT_COMMAND}`; inspect individual checks with `cargo xtask checks`"),
    )]
}

fn validate_document(document: &crate::repository::markdown::Document) -> Vec<Violation> {
    let actual_headings = document_second_level_headings(document);
    let expected_headings: Vec<_> = PULL_REQUEST_HEADINGS
        .iter()
        .map(ToString::to_string)
        .collect();
    let mut violations = Vec::new();
    let canonical = canonical_source();
    if document.body() != canonical {
        violations.push(pull_request_violation(
            first_difference_line(document.body(), &canonical),
            "pull request template source differs from its checker-owned canonical shape",
            "restore the canonical prompts, blank author fields, tables, and checklist without extra content",
        ));
    }
    if let Some(line) = document.first_raw_html_between(0, document.body().lines().count() + 1) {
        violations.push(pull_request_violation(
            line,
            "pull request template uses raw HTML around policy-bearing content",
            "express every required prompt and verification row with plain Markdown",
        ));
    }
    if document.body().contains("~~") {
        violations.push(pull_request_violation(
            1,
            "pull request template uses strikethrough around policy-bearing content",
            "keep every canonical prompt visibly active and remove strikethrough markup",
        ));
    }
    if actual_headings != expected_headings {
        violations.push(pull_request_violation(
            first_second_level(document.body()),
            "pull request section names or order have drifted from the review contract",
            "restore TL;DR, goal, scope, design, architecture, verification, risk, and review sections in canonical order",
        ));
    }

    for (section, expected) in CANONICAL_SECTION_TEXT {
        if normalized_visible_section_text(document, section).as_deref() != Some(expected) {
            violations.push(pull_request_violation(
                line_or_start(heading_line(document.body(), section)),
                format!("pull request template section `{section}` differs from its canonical visible prompt"),
                "restore the complete visible prompt without semantic qualifiers or extra policy text",
            ));
        }
    }
    violations.extend(verification_table_violations(document));
    violations.extend(review_checklist_violations(document));
    violations
}

pub(super) fn canonical_source() -> String {
    format!(
        r#"## TL;DR

State the one outcome delivered, the highest material risk, and the verification result.

## Goal

Describe the problem and observable result.

**Issue:**
**Milestone:**

## Scope and exclusions

- Included:
- Deliberately excluded:

## Design and compatibility

Describe material decisions, assumptions, contracts, event compatibility,
service ownership, data meaning, deployment profiles, and ADRs.

- Unresolved questions:
- Known limitations:
- Follow-up work:

## Architecture

- Owning capability and package role:
- Inbound caller or adapter:
- Consumed ports and outbound adapters:
- Internal dependency edges added, removed, or reclassified:
- End-to-end behavior proved:

## Verification

| Command or check | Outcome | Evidence or reason not run |
|---|---|---|
| `cargo xtask repository` | | |
| `cargo xtask architecture` | | |
| `{}` | | |

**Checks not run and blocker:**

**Residual risk:**

## Risk and operations

Describe security, market-data integrity, model trust, event delivery/replay,
dry-run execution boundaries, migration, rollout, rollback, deployment
portability, and operational effects. Write “None” only after reviewing each area.

## Review checklist

{}
"#,
        DEFAULT_COMMAND,
        PULL_REQUEST_CHECKLIST.join("\n")
    )
}

fn first_difference_line(actual: &str, expected: &str) -> usize {
    let actual_lines: Vec<_> = actual.lines().collect();
    let expected_lines: Vec<_> = expected.lines().collect();
    let shared = actual_lines.len().min(expected_lines.len());
    for index in 0..shared {
        if actual_lines[index] != expected_lines[index] {
            return index + 1;
        }
    }
    shared + 1
}

fn verification_table_violations(
    document: &crate::repository::markdown::Document,
) -> Vec<Violation> {
    let Some((start, end)) = section_lines(document, "Verification") else {
        return vec![pull_request_violation(
            1,
            "pull request template has no verification section",
            "restore the canonical verification section and table",
        )];
    };
    let rows: Vec<_> = document
        .table_rows()
        .iter()
        .filter(|row| row.line > start && row.line < end)
        .collect();
    let mut violations = Vec::new();
    let expected_headers: Vec<_> = VERIFICATION_HEADERS
        .iter()
        .map(ToString::to_string)
        .collect();
    let actual_headers = rows.first().map_or_else(Vec::new, |row| {
        row.cells
            .iter()
            .map(|cell| cell.trim().to_owned())
            .collect()
    });
    if actual_headers != expected_headers {
        violations.push(pull_request_violation(
            start,
            "verification table header differs from the canonical three-column contract",
            "restore `Command or check`, `Outcome`, and `Evidence or reason not run`",
        ));
    }

    let lines: Vec<_> = document.body().lines().collect();
    let data_rows = rows.get(1..).map_or(&[][..], |rows| rows);
    if data_rows.len() != REQUIRED_VERIFICATION_COMMANDS.len() {
        violations.push(pull_request_violation(
            start,
            format!(
                "verification table contains {} data rows; expected {}",
                data_rows.len(),
                REQUIRED_VERIFICATION_COMMANDS.len()
            ),
            "keep exactly the repository, architecture, and aggregate verification rows",
        ));
    }
    for (index, expected) in REQUIRED_VERIFICATION_COMMANDS.iter().enumerate() {
        let Some(row) = data_rows.get(index) else {
            continue;
        };
        if row.cells.len() != VERIFICATION_HEADERS.len() {
            violations.push(pull_request_violation(
                row.line,
                "verification row does not contain exactly three cells",
                "keep the command, outcome, and evidence cells",
            ));
            continue;
        }
        let actual = lines
            .get(row.line.saturating_sub(1))
            .and_then(|line| line.trim().strip_prefix('|'))
            .and_then(|source| source.split_once('|').map(|(cell, _)| cell.trim()))
            .and_then(exact_inline_code);
        if actual != Some(*expected) {
            violations.push(pull_request_violation(
                row.line,
                format!(
                    "verification row {} must begin with exact inline command `{expected}`",
                    index + 1
                ),
                "restore the canonical command in an exact inline-code first cell",
            ));
        }
        if row
            .cells
            .get(1..)
            .is_some_and(|cells| cells.iter().any(|cell| !cell.trim().is_empty()))
        {
            violations.push(pull_request_violation(
                row.line,
                format!(
                    "verification row {} contains a pre-populated outcome or evidence claim",
                    index + 1
                ),
                "leave outcome and evidence cells empty for the pull request author",
            ));
        }
    }
    let source_lines = visible_section_source_lines(document, "Verification");
    for prompt in VERIFICATION_PROMPTS {
        if source_lines
            .iter()
            .filter(|line| line.as_str() == prompt)
            .count()
            != 1
        {
            violations.push(pull_request_violation(
                start,
                format!("verification section must contain exact prompt `{prompt}` once"),
                "restore the canonical blocker and residual-risk prompts",
            ));
        }
    }
    violations
}

fn exact_inline_code(source: &str) -> Option<&str> {
    let code = source.strip_prefix('`')?.strip_suffix('`')?;
    (!code.is_empty() && !code.contains('`')).then_some(code)
}

fn review_checklist_violations(document: &crate::repository::markdown::Document) -> Vec<Violation> {
    let expected: Vec<_> = PULL_REQUEST_CHECKLIST
        .iter()
        .map(ToString::to_string)
        .collect();
    let actual: Vec<_> = visible_section_source_lines(document, "Review checklist")
        .into_iter()
        .filter(|line| line.starts_with("- ["))
        .collect();
    if actual == expected {
        return Vec::new();
    }
    vec![pull_request_violation(
        line_or_start(heading_line(document.body(), "Review checklist")),
        "review checklist has drifted from the enforced review contract",
        "restore the nine canonical unchecked review prompts in their documented order",
    )]
}

#[cfg(test)]
pub(super) fn validate_fragment(
    document: &crate::repository::markdown::Document,
) -> Vec<Violation> {
    validate_document(document)
}

#[cfg(test)]
pub(super) fn validate_contributor_fragment(
    document: &crate::repository::markdown::Document,
) -> Vec<Violation> {
    verification_section_violations(
        document,
        VERIFICATION_DOCUMENTS[0].0,
        VERIFICATION_DOCUMENTS[0].1,
    )
}

#[cfg(test)]
pub(super) fn validate_foundation_fragment(
    document: &crate::repository::markdown::Document,
) -> Vec<Violation> {
    verification_section_violations(
        document,
        VERIFICATION_DOCUMENTS[1].0,
        VERIFICATION_DOCUMENTS[1].1,
    )
}
