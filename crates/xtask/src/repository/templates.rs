mod issue;
mod pull_request;

#[cfg(test)]
mod tests;

use super::{ISSUE_POLICY, PULL_REQUEST_POLICY, Repository, VERIFICATION_POLICY, Violation};

const CONTRIBUTING: &str = "CONTRIBUTING.md";
const CAPABILITY_ISSUE_TEMPLATE: &str = ".github/ISSUE_TEMPLATE/capability.yml";
const BUG_ISSUE_TEMPLATE: &str = ".github/ISSUE_TEMPLATE/bug.yml";
const ISSUE_TEMPLATE_CONFIG: &str = ".github/ISSUE_TEMPLATE/config.yml";
const PULL_REQUEST_TEMPLATE: &str = ".github/PULL_REQUEST_TEMPLATE.md";

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let mut violations = issue::check(repository);
    violations.extend(pull_request::check(repository));
    violations
}

fn section_lines(
    document: &crate::repository::markdown::Document,
    name: &str,
) -> Option<(usize, usize)> {
    let headings = document.headings();
    let (index, heading) = headings
        .iter()
        .enumerate()
        .find(|(_, heading)| heading.level == 2 && heading.text.trim() == name)?;
    let end = headings[index + 1..]
        .iter()
        .find(|candidate| candidate.level <= 2)
        .map_or_else(
            || document.body().lines().count() + 1,
            |candidate| candidate.line,
        );
    Some((heading.line, end))
}

fn document_second_level_headings(document: &crate::repository::markdown::Document) -> Vec<String> {
    document
        .headings()
        .iter()
        .filter(|heading| heading.level == 2)
        .map(|heading| heading.text.trim().to_owned())
        .collect()
}

fn heading_line(body: &str, heading: &str) -> Option<usize> {
    let marker = format!("## {heading}");
    body.lines()
        .position(|line| line == marker)
        .map(|index| index + 1)
}

fn first_second_level(body: &str) -> usize {
    body.lines()
        .position(|line| line.starts_with("## "))
        .map_or(1, |index| index + 1)
}

fn line_or_start(line: Option<usize>) -> usize {
    line.unwrap_or(1)
}

fn visible_section_source_lines(
    document: &crate::repository::markdown::Document,
    heading: &str,
) -> Vec<String> {
    let Some((start, end)) = section_lines(document, heading) else {
        return Vec::new();
    };
    document
        .body()
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line_number = index + 1;
            (line_number > start
                && line_number < end
                && !line.trim().is_empty()
                && document.line_is_visible(line_number))
            .then(|| line.trim().to_owned())
        })
        .collect()
}

fn normalized_visible_section_text(
    document: &crate::repository::markdown::Document,
    heading: &str,
) -> Option<String> {
    let (start, end) = section_lines(document, heading)?;
    Some(
        (start + 1..end)
            .filter_map(|line| document.visible_line_text(line))
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn issue_violation(
    path: impl Into<String>,
    line: usize,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation::new(path, line, "template.issue", message, fix, ISSUE_POLICY)
}

fn pull_request_violation(
    line: usize,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    pull_request_path_violation(PULL_REQUEST_TEMPLATE, line, message, fix)
}

fn pull_request_path_violation(
    path: impl Into<String>,
    line: usize,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation::new(
        path,
        line,
        "template.pull-request",
        message,
        fix,
        PULL_REQUEST_POLICY,
    )
}

fn verification_policy_violation(
    line: usize,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation::new(
        CONTRIBUTING,
        line,
        "template.verification-policy",
        message,
        fix,
        VERIFICATION_POLICY,
    )
}
