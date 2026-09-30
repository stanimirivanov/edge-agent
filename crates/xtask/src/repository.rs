mod adr;
mod markdown;
mod milestones;
mod templates;

#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use markdown::Document;

pub(super) const DOCUMENTATION_POLICY: &str = "CONTRIBUTING.md#documentation";
pub(super) const ADR_POLICY: &str = "docs/decisions/README.md#naming-and-lifecycle";
pub(super) const ISSUE_POLICY: &str = "CONTRIBUTING.md#issue-timing-and-structure";
pub(super) const PULL_REQUEST_POLICY: &str = "CONTRIBUTING.md#pull-request-description";
pub(super) const MILESTONE_POLICY: &str = "docs/roadmap/milestones.md#planning-rules";
pub(super) const VERIFICATION_POLICY: &str =
    "CONTRIBUTING.md#verification-and-constrained-environments";
const REPOSITORY_POLICY: &str = "docs/development/harness.md#repository-policy";
const POLICY_LINKS: [(&str, &str); 7] = [
    ("documentation", DOCUMENTATION_POLICY),
    ("architecture decisions", ADR_POLICY),
    ("issue authoring", ISSUE_POLICY),
    ("pull request authoring", PULL_REQUEST_POLICY),
    ("verification", VERIFICATION_POLICY),
    ("milestones", MILESTONE_POLICY),
    ("repository harness", REPOSITORY_POLICY),
];

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

#[derive(Debug)]
pub(crate) struct RepositoryReport {
    document_count: usize,
    violations: Vec<Violation>,
}

impl RepositoryReport {
    pub(crate) const fn document_count(&self) -> usize {
        self.document_count
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.violations.is_empty()
    }

    pub(crate) fn violations(&self) -> &[Violation] {
        &self.violations
    }
}

#[derive(Debug)]
pub(crate) enum RepositoryCheckError {
    Operational(String),
}

impl fmt::Display for RepositoryCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operational(message) => formatter.write_str(message),
        }
    }
}

impl Error for RepositoryCheckError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Violation {
    path: String,
    line: usize,
    rule: &'static str,
    message: String,
    fix: String,
    policy: &'static str,
}

impl Violation {
    pub(super) fn new(
        path: impl Into<String>,
        line: usize,
        rule: &'static str,
        message: impl Into<String>,
        fix: impl Into<String>,
        policy: &'static str,
    ) -> Self {
        Self {
            path: path.into(),
            line,
            rule,
            message: message.into(),
            fix: fix.into(),
            policy,
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}: [{}] {}; fix: {}; policy: {}",
            self.path, self.line, self.rule, self.message, self.fix, self.policy
        )
    }
}

pub(super) struct Repository {
    root: PathBuf,
    documents: BTreeMap<String, Document>,
}

impl Repository {
    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn document(&self, path: &str) -> Option<&Document> {
        self.documents.get(path)
    }
}

/// Checks deterministic repository and documentation policy.
pub(crate) fn check() -> Result<RepositoryReport, RepositoryCheckError> {
    check_at(Path::new(WORKSPACE_ROOT))
}

fn check_at(root: &Path) -> Result<RepositoryReport, RepositoryCheckError> {
    let repository = markdown::load(root).map_err(RepositoryCheckError::Operational)?;
    let document_count = repository.documents.len();
    let mut violations = check_policy_links(&repository);
    violations.extend(markdown::check_links(&repository));
    violations.extend(markdown::check_tldr(&repository));
    violations.extend(adr::check(&repository));
    violations.extend(milestones::check(&repository));
    violations.extend(templates::check(&repository));
    violations.sort_by(|left, right| {
        (&left.path, left.line, left.rule, &left.message).cmp(&(
            &right.path,
            right.line,
            right.rule,
            &right.message,
        ))
    });

    Ok(RepositoryReport {
        document_count,
        violations,
    })
}

fn check_policy_links(repository: &Repository) -> Vec<Violation> {
    POLICY_LINKS
        .iter()
        .filter_map(|(name, target)| {
            let Some((path, fragment)) = target.split_once('#') else {
                return Some(policy_link_violation(
                    *target,
                    format!("checker-owned {name} policy link has no heading fragment"),
                ));
            };
            let Some(document) = repository.document(path) else {
                return Some(policy_link_violation(
                    path,
                    format!("checker-owned {name} policy document is missing"),
                ));
            };
            if document
                .headings()
                .iter()
                .any(|heading| heading.anchor == fragment)
            {
                None
            } else {
                Some(policy_link_violation(
                    path,
                    format!("checker-owned {name} policy anchor `#{fragment}` does not exist"),
                ))
            }
        })
        .collect()
}

fn policy_link_violation(path: impl Into<String>, message: impl Into<String>) -> Violation {
    Violation::new(
        path,
        1,
        "repository.policy-link",
        message,
        "restore the canonical heading or intentionally update the checker-owned policy link",
        REPOSITORY_POLICY,
    )
}
