use std::fs;

use super::{
    BUG_ISSUE_TEMPLATE, CAPABILITY_ISSUE_TEMPLATE, CONTRIBUTING, ISSUE_TEMPLATE_CONFIG, Repository,
    Violation, issue_violation, section_lines,
};

const CANONICAL_ISSUE_BODY: &str = r#"**Milestone:** MNN - Outcome

## Goal

Describe the problem and observable result.

## Scope

- Included behavior and boundaries.

## Design decisions

- Assumptions, constraints, compatibility effects, and ADR links.

## Acceptance criteria

- [ ] Observable behavior and verification evidence.
- [ ] Relevant failure or negative behavior.
- [ ] Documentation and operational effects.

## Out of scope

- Explicit exclusions and deferred work.
"#;

const CAPABILITY_FORM: &[&str] = &[
    "name: Capability proposal",
    "description: Propose one independently reviewable product or engineering outcome.",
    "title: \"\"",
    "labels: [enhancement]",
    "body:",
    "  - type: input",
    "    id: milestone",
    "    attributes:",
    "      label: Milestone",
    "      description: Exact title in the form MNN - Outcome.",
    "    validations:",
    "      required: true",
    "  - type: textarea",
    "    id: goal",
    "    attributes:",
    "      label: Goal",
    "      description: State the problem and observable result.",
    "    validations:",
    "      required: true",
    "  - type: textarea",
    "    id: scope",
    "    attributes:",
    "      label: Scope",
    "      description: Define included behavior and boundaries.",
    "    validations:",
    "      required: true",
    "  - type: textarea",
    "    id: decisions",
    "    attributes:",
    "      label: Design decisions",
    "      description: Record assumptions, constraints, compatibility effects, and ADR links.",
    "    validations:",
    "      required: true",
    "  - type: textarea",
    "    id: acceptance",
    "    attributes:",
    "      label: Acceptance criteria",
    "      description: Use verifiable outcomes, including important negative behavior.",
    "      placeholder: \"- [ ] ...\"",
    "    validations:",
    "      required: true",
    "  - type: textarea",
    "    id: exclusions",
    "    attributes:",
    "      label: Out of scope",
    "      description: State explicit exclusions and deferred work.",
    "    validations:",
    "      required: true",
];

const BUG_FORM: &[&str] = &[
    "name: Bug report",
    "description: Report reproducible incorrect behavior without disclosing a vulnerability.",
    "title: \"\"",
    "labels: [bug]",
    "body:",
    "  - type: markdown",
    "    attributes:",
    "      value: For suspected vulnerabilities, follow SECURITY.md instead of opening a public issue.",
    "  - type: input",
    "    id: milestone",
    "    attributes:",
    "      label: Milestone",
    "      description: Use the exact MNN - Outcome title when known.",
    "    validations:",
    "      required: false",
    "  - type: textarea",
    "    id: observed",
    "    attributes:",
    "      label: Observed behavior",
    "      description: Include inputs, versions, timestamps, and redacted evidence.",
    "    validations:",
    "      required: true",
    "  - type: textarea",
    "    id: expected",
    "    attributes:",
    "      label: Expected behavior",
    "    validations:",
    "      required: true",
    "  - type: textarea",
    "    id: reproduction",
    "    attributes:",
    "      label: Minimal reproduction",
    "    validations:",
    "      required: true",
    "  - type: textarea",
    "    id: environment",
    "    attributes:",
    "      label: Environment",
    "      description: Revision, operating system, runtime, and relevant adapter versions.",
    "    validations:",
    "      required: true",
    "  - type: checkboxes",
    "    id: safety",
    "    attributes:",
    "      label: Safety check",
    "      options:",
    "        - label: I removed credentials, personal data, and licensed market-data payloads.",
    "          required: true",
    "        - label: This report does not contain vulnerability details.",
    "          required: true",
];

const ISSUE_CONFIG: &[&str] = &[
    "blank_issues_enabled: false",
    "contact_links:",
    "  - name: Security vulnerability",
    "    url: https://github.com/stanimirivanov/edge-agent/security/advisories/new",
    "    about: Report security issues privately. Do not include vulnerability details in a public issue.",
];
const REGISTERED_ISSUE_TEMPLATE_NAMES: &[&str] = &["capability.yml", "bug.yml", "config.yml"];
const ISSUE_TEMPLATE_DIRECTORY: &str = ".github/ISSUE_TEMPLATE";
const ISSUE_TEMPLATE_DIRECTORIES: &[&str] = &[
    ISSUE_TEMPLATE_DIRECTORY,
    "ISSUE_TEMPLATE",
    "docs/ISSUE_TEMPLATE",
];

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let mut violations = inventory_violations(repository);
    violations.extend(match repository.document(CONTRIBUTING) {
        Some(policy) => validate_contributor_policy(policy),
        None => vec![issue_violation(
            CONTRIBUTING,
            1,
            "canonical contribution policy is missing",
            "restore the contributor guide and its fenced canonical issue body",
        )],
    });
    violations.extend([
        (
            CAPABILITY_ISSUE_TEMPLATE,
            CAPABILITY_FORM,
            "capability issue form",
        ),
        (BUG_ISSUE_TEMPLATE, BUG_FORM, "bug issue form"),
        (ISSUE_TEMPLATE_CONFIG, ISSUE_CONFIG, "issue template routing"),
    ]
    .into_iter()
    .filter_map(|(path, expected, description)| {
        let source = fs::read_to_string(repository.root().join(path));
        match source {
            Ok(body) => first_line_difference(&body, expected).map(|line| {
                issue_violation(
                    path,
                    line,
                    format!("{description} differs from its checker-owned contract"),
                    "restore the canonical ordered fields, prompts, validations, labels, and security routing, or review and update the checker contract intentionally",
                )
            }),
            Err(error) => Some(issue_violation(
                path,
                1,
                format!("could not read {description}: {error}"),
                "restore the canonical checked-in GitHub issue form",
            )),
        }
    })
    .collect::<Vec<_>>());
    violations
}

fn inventory_violations(repository: &Repository) -> Vec<Violation> {
    let mut violations = Vec::new();
    for relative_directory in ISSUE_TEMPLATE_DIRECTORIES {
        let directory = repository.root().join(relative_directory);
        let mut entries = match fs::read_dir(&directory) {
            Ok(entries) => match entries.collect::<Result<Vec<_>, _>>() {
                Ok(entries) => entries,
                Err(error) => {
                    violations.push(issue_violation(
                        *relative_directory,
                        1,
                        format!("could not inventory GitHub issue templates: {error}"),
                        "restore readable issue-template locations containing only the registered forms and routing config",
                    ));
                    continue;
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                violations.push(issue_violation(
                    *relative_directory,
                    1,
                    format!("could not inventory GitHub issue templates: {error}"),
                    "restore readable issue-template locations containing only the registered forms and routing config",
                ));
                continue;
            }
        };
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let path = format!("{relative_directory}/{name}");
            match entry.file_type() {
                Ok(file_type)
                    if *relative_directory == ISSUE_TEMPLATE_DIRECTORY
                        && file_type.is_file()
                        && REGISTERED_ISSUE_TEMPLATE_NAMES.contains(&name.as_ref()) =>
                {
                    {}
                }
                Ok(_) => violations.push(issue_violation(
                    path,
                    1,
                    format!("unregistered GitHub issue-template entry `{name}` can bypass the governed forms"),
                    "remove the entry or intentionally register and enforce it alongside capability.yml, bug.yml, and config.yml under .github/ISSUE_TEMPLATE",
                )),
                Err(error) => violations.push(issue_violation(
                    path,
                    1,
                    format!("could not inspect GitHub issue-template entry `{name}`: {error}"),
                    "restore a readable regular file or remove the unregistered entry",
                )),
            }
        }
    }
    violations
}

fn validate_contributor_policy(document: &crate::repository::markdown::Document) -> Vec<Violation> {
    let Some((start, end)) = section_lines(document, "Issue timing and structure") else {
        return vec![issue_violation(
            CONTRIBUTING,
            1,
            "contributor guide has no canonical issue-policy section",
            "restore `## Issue timing and structure` and its fenced `markdown` issue body",
        )];
    };
    let blocks: Vec<_> = document
        .code_blocks()
        .iter()
        .filter(|block| block.line > start && block.line < end && block.info.trim() == "markdown")
        .collect();
    if blocks.len() != 1 {
        return vec![issue_violation(
            CONTRIBUTING,
            start,
            format!(
                "issue-policy section contains {} fenced `markdown` bodies; expected one",
                blocks.len()
            ),
            "restore one canonical fenced implementation-issue body",
        )];
    }
    let Some(block) = blocks.first() else {
        return Vec::new();
    };
    if block.body == CANONICAL_ISSUE_BODY {
        return Vec::new();
    }
    vec![issue_violation(
        CONTRIBUTING,
        block.line,
        "canonical contributor issue body differs from the checker-owned contract",
        "restore the milestone field, five sections, and exact author prompts in canonical order",
    )]
}

fn first_line_difference(body: &str, expected: &[&str]) -> Option<usize> {
    let actual: Vec<_> = body.lines().collect();
    let shared = actual.len().min(expected.len());
    for index in 0..shared {
        if actual[index] != expected[index] {
            return Some(index + 1);
        }
    }
    (actual.len() != expected.len()).then_some(shared + 1)
}

#[cfg(test)]
pub(super) fn expected_form(path: &str) -> Option<&'static [&'static str]> {
    match path {
        CAPABILITY_ISSUE_TEMPLATE => Some(CAPABILITY_FORM),
        BUG_ISSUE_TEMPLATE => Some(BUG_FORM),
        ISSUE_TEMPLATE_CONFIG => Some(ISSUE_CONFIG),
        _ => None,
    }
}

#[cfg(test)]
pub(super) const fn canonical_issue_body() -> &'static str {
    CANONICAL_ISSUE_BODY
}

#[cfg(test)]
pub(super) fn validate_contributor_fragment(
    document: &crate::repository::markdown::Document,
) -> Vec<Violation> {
    validate_contributor_policy(document)
}
