use crate::repository::markdown::{Document, parse_fragment};

use super::{ADR_INDEX, METADATA_FIELDS, REQUIRED_SECTIONS, Repository, Violation, adr_violation};

const TEMPLATE_HEADING: &str = "Template";
const TEMPLATE_METADATA: [&str; 6] = [
    "- Status: Proposed",
    "- Date: YYYY-MM-DD",
    "- Milestone: MNN - Outcome",
    "- Deciders:",
    "- Supersedes:",
    "- Superseded by:",
];

pub(super) fn validate(repository: &Repository) -> Vec<Violation> {
    let Some(index) = repository.document(ADR_INDEX) else {
        return Vec::new();
    };
    let headings: Vec<_> = index
        .headings()
        .iter()
        .enumerate()
        .filter(|(_, heading)| heading.level == 2 && heading.text.trim() == TEMPLATE_HEADING)
        .collect();
    if headings.len() != 1 {
        return vec![adr_violation(
            ADR_INDEX,
            headings.first().map_or(1, |(_, heading)| heading.line),
            "adr.template",
            "decision index must contain exactly one rendered `Template` section",
            "restore the canonical fenced Markdown decision template",
        )];
    }
    let (heading_index, heading) = headings[0];
    let end = index.headings()[heading_index + 1..]
        .iter()
        .find(|candidate| candidate.level <= 2)
        .map_or_else(
            || index.body().lines().count() + 1,
            |candidate| candidate.line,
        );
    let mut violations = Vec::new();
    if let Some(line) = index.first_raw_html_between(heading.line, end) {
        violations.push(adr_violation(
            ADR_INDEX,
            line,
            "adr.template",
            "canonical decision template uses raw HTML around policy-bearing content",
            "express the template heading and fenced body with plain Markdown",
        ));
    }
    let blocks: Vec<_> = index
        .code_blocks()
        .iter()
        .filter(|block| {
            block.line > heading.line && block.line < end && block.info.trim() == "markdown"
        })
        .collect();
    if blocks.len() != 1 {
        violations.push(adr_violation(
            ADR_INDEX,
            heading.line,
            "adr.template",
            "Template section must contain exactly one fenced `markdown` decision template",
            "restore one canonical fenced Markdown block below `## Template`",
        ));
        return violations;
    }
    let Some(block) = blocks.first() else {
        return violations;
    };
    let template = parse_fragment(ADR_INDEX, block.body.trim_end());
    violations.extend(validate_document(&template, block.line + 1));
    violations
}

fn validate_document(document: &Document, source_line: usize) -> Vec<Violation> {
    let mut violations = Vec::new();
    let title_is_canonical = document.headings().first().is_some_and(|heading| {
        heading.level == 1 && heading.line == 1 && heading.text.trim() == "ADR-NNNN: Decision title"
    });
    if !title_is_canonical {
        violations.push(template_violation(
            source_line,
            "canonical decision template has no placeholder title",
            "restore `# ADR-NNNN: Decision title` as the first line",
        ));
    }

    let first_section = document
        .headings()
        .iter()
        .find(|heading| heading.level == 2)
        .map_or_else(
            || document.body().lines().count() + 1,
            |heading| heading.line,
        );
    let actual_metadata: Vec<_> = document
        .body()
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line_number = index + 1;
            (line_number > 1
                && line_number < first_section
                && !line.trim().is_empty()
                && document.line_is_visible(line_number))
            .then(|| line.trim())
        })
        .collect();
    if actual_metadata != TEMPLATE_METADATA {
        violations.push(template_violation(
            source_line + 1,
            format!(
                "canonical decision template metadata does not match `{}`",
                METADATA_FIELDS.join(", ")
            ),
            "restore the six canonical list items and placeholders in documented order",
        ));
    }

    let actual_sections: Vec<_> = document
        .headings()
        .iter()
        .filter(|heading| heading.level == 2)
        .map(|heading| heading.text.trim())
        .collect();
    if actual_sections != REQUIRED_SECTIONS {
        violations.push(template_violation(
            source_line,
            "canonical decision template sections have drifted",
            format!(
                "restore `{}` in exact level-two order",
                REQUIRED_SECTIONS.join(", ")
            ),
        ));
    }
    if document
        .first_raw_html_between(0, document.body().lines().count() + 1)
        .is_some()
    {
        violations.push(template_violation(
            source_line,
            "canonical decision template contains raw HTML",
            "express all required template fields with plain Markdown",
        ));
    }
    violations
}

fn template_violation(
    line: usize,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    adr_violation(ADR_INDEX, line, "adr.template", message, fix)
}
