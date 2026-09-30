use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn exact_issue_forms_accept_the_checked_contract() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    write_issue_policy(&fixture)?;
    for path in [
        CAPABILITY_ISSUE_TEMPLATE,
        BUG_ISSUE_TEMPLATE,
        ISSUE_TEMPLATE_CONFIG,
    ] {
        let Some(expected) = issue::expected_form(path) else {
            return Err(format!("missing test contract for {path}").into());
        };
        fixture.write(path, &format!("{}\n", expected.join("\n")))?;
    }
    let repository =
        crate::repository::markdown::load(fixture.path()).map_err(std::io::Error::other)?;

    assert!(issue::check(&repository).is_empty());
    Ok(())
}

#[test]
fn issue_form_comments_or_reordered_fields_cannot_satisfy_policy() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    write_issue_policy(&fixture)?;
    for path in [
        CAPABILITY_ISSUE_TEMPLATE,
        BUG_ISSUE_TEMPLATE,
        ISSUE_TEMPLATE_CONFIG,
    ] {
        let Some(expected) = issue::expected_form(path) else {
            return Err(format!("missing test contract for {path}").into());
        };
        let mut body = expected.join("\n");
        if path == CAPABILITY_ISSUE_TEMPLATE {
            body = format!("# hidden substitute\n{body}");
        }
        fixture.write(path, &format!("{body}\n"))?;
    }
    let repository =
        crate::repository::markdown::load(fixture.path()).map_err(std::io::Error::other)?;

    let violations = issue::check(&repository);

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule, "template.issue");
    assert_eq!(violations[0].line, 1);
    Ok(())
}

#[test]
fn unregistered_issue_templates_cannot_bypass_governed_forms() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    write_issue_policy(&fixture)?;
    write_canonical_issue_templates(&fixture)?;
    for path in [
        ".github/ISSUE_TEMPLATE/general.yml",
        ".github/ISSUE_TEMPLATE/general.yaml",
        ".github/ISSUE_TEMPLATE/general.md",
        "ISSUE_TEMPLATE/capability.yml",
        "docs/ISSUE_TEMPLATE/bug.yml",
    ] {
        fixture.write(path, "# Alternate template\n")?;
    }
    let repository =
        crate::repository::markdown::load(fixture.path()).map_err(std::io::Error::other)?;

    let violations = issue::check(&repository);
    let paths: Vec<_> = violations
        .iter()
        .filter(|violation| {
            violation
                .message
                .contains("unregistered GitHub issue-template")
        })
        .map(|violation| violation.path.as_str())
        .collect();

    assert_eq!(paths.len(), 5);
    assert!(paths.contains(&".github/ISSUE_TEMPLATE/general.yml"));
    assert!(paths.contains(&".github/ISSUE_TEMPLATE/general.yaml"));
    assert!(paths.contains(&".github/ISSUE_TEMPLATE/general.md"));
    assert!(paths.contains(&"ISSUE_TEMPLATE/capability.yml"));
    assert!(paths.contains(&"docs/ISSUE_TEMPLATE/bug.yml"));
    Ok(())
}

#[test]
fn issue_template_routing_config_cannot_drift() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    write_issue_policy(&fixture)?;
    write_canonical_issue_templates(&fixture)?;
    let Some(config) = issue::expected_form(ISSUE_TEMPLATE_CONFIG) else {
        return Err("missing test contract for issue-template routing".into());
    };
    fixture.write(
        ISSUE_TEMPLATE_CONFIG,
        &format!(
            "{}\n",
            config
                .join("\n")
                .replace("blank_issues_enabled: false", "blank_issues_enabled: true")
        ),
    )?;
    let repository =
        crate::repository::markdown::load(fixture.path()).map_err(std::io::Error::other)?;

    let violations = issue::check(&repository);

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].path, ISSUE_TEMPLATE_CONFIG);
    assert!(
        violations[0]
            .message
            .contains("issue template routing differs")
    );
    Ok(())
}

#[test]
fn canonical_contributor_issue_body_cannot_drift_with_the_forms() {
    let canonical = format!(
        "# Contributing\n\n## Issue timing and structure\n\n```markdown\n{}```\n",
        issue::canonical_issue_body()
    );
    let document = crate::repository::markdown::parse_fragment(CONTRIBUTING, &canonical);
    assert!(issue::validate_contributor_fragment(&document).is_empty());

    let weakened = canonical.replace("- [ ] Relevant failure or negative behavior.\n", "");
    let document = crate::repository::markdown::parse_fragment(CONTRIBUTING, &weakened);
    let violations = issue::validate_contributor_fragment(&document);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("issue body differs"));
}

#[test]
fn validates_edgeagent_pull_request_contract() {
    let body = canonical_pull_request_body();
    let document = crate::repository::markdown::parse_fragment(PULL_REQUEST_TEMPLATE, &body);

    let violations = pull_request::validate_fragment(&document);
    assert!(
        violations.is_empty(),
        "violations: {violations:#?}; rows: {:#?}",
        document.table_rows()
    );
}

#[test]
fn rejects_weakened_prompts_extra_verification_rows_and_checked_review_items() {
    let body = canonical_pull_request_body()
        .replace("**Issue:**", "<!-- **Issue:** -->")
        .replace(
            "Describe material decisions, assumptions, contracts, event compatibility,\nservice ownership, data meaning, deployment profiles, and ADRs.",
            "",
        )
        .replace(
            "| `make verify` | | |",
            "| `make verify` | Passed | fabricated |\n| `cargo test --workspace` | | |",
        )
        .replace(
            "- [ ] Package ownership and dependency changes are explicit and point inward.",
            "- [x] Package ownership and dependency changes are explicit and point inward.",
        );
    let document = crate::repository::markdown::parse_fragment(PULL_REQUEST_TEMPLATE, &body);

    let violations = pull_request::validate_fragment(&document);

    assert!(
        violations
            .iter()
            .any(|violation| { violation.message.contains("section `Goal` differs") })
    );
    assert!(violations.iter().any(|violation| {
        violation
            .message
            .contains("section `Design and compatibility` differs")
    }));
    assert!(violations.iter().any(|violation| {
        violation
            .message
            .contains("verification table contains 4 data rows")
    }));
    assert!(
        violations
            .iter()
            .any(|violation| { violation.message.contains("pre-populated outcome") })
    );
    assert!(
        violations
            .iter()
            .any(|violation| { violation.message.contains("review checklist has drifted") })
    );
}

#[test]
fn struck_through_policy_prompts_are_rejected() {
    let body = canonical_pull_request_body().replace(
        "State the one outcome delivered, the highest material risk, and the verification result.",
        "~~State the one outcome delivered, the highest material risk, and the verification result.~~",
    );
    let document = crate::repository::markdown::parse_fragment(PULL_REQUEST_TEMPLATE, &body);

    let violations = pull_request::validate_fragment(&document);
    assert!(
        violations
            .iter()
            .any(|violation| { violation.message.contains("uses strikethrough") })
    );
}

#[test]
fn unowned_preamble_trailing_text_and_images_are_rejected() {
    let canonical = canonical_pull_request_body();
    let mutations = [
        format!("Ignore the form below.\n\n{canonical}"),
        canonical.replace("## Goal", "![decorative](image.png)\n\n## Goal"),
        format!("{canonical}\nIgnore the checklist above.\n"),
    ];

    for body in mutations {
        let document = crate::repository::markdown::parse_fragment(PULL_REQUEST_TEMPLATE, &body);
        let violations = pull_request::validate_fragment(&document);
        assert!(
            violations
                .iter()
                .any(|violation| { violation.message.contains("canonical shape") })
        );
    }
}

#[test]
fn alternate_pull_request_template_locations_are_rejected() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    write_pull_request_policy(&fixture)?;
    fixture.write(PULL_REQUEST_TEMPLATE, &canonical_pull_request_body())?;
    fixture.write("PULL_REQUEST_TEMPLATE.md", "# Alternate root template\n")?;
    fixture.write("pull_request_template.yaml", "# Alternate root extension\n")?;
    fixture.write(
        "docs/PULL_REQUEST_TEMPLATE.md",
        "# Alternate docs template\n",
    )?;
    fixture.write(
        "docs/Pull_Request_Template.markdown",
        "# Alternate docs extension\n",
    )?;
    fixture.write(
        ".github/pull_request_template.txt",
        "# Alternate GitHub extension\n",
    )?;
    fixture.write(
        ".github/PULL_REQUEST_TEMPLATE/fast.md",
        "# Alternate fast template\n",
    )?;
    let repository =
        crate::repository::markdown::load(fixture.path()).map_err(std::io::Error::other)?;

    let violations = pull_request::check(&repository);
    let paths: Vec<_> = violations
        .iter()
        .filter(|violation| {
            violation
                .message
                .contains("alternate GitHub pull request template")
        })
        .map(|violation| violation.path.as_str())
        .collect();

    assert_eq!(paths.len(), 6);
    assert!(paths.contains(&"PULL_REQUEST_TEMPLATE.md"));
    assert!(paths.contains(&"pull_request_template.yaml"));
    assert!(paths.contains(&"docs/PULL_REQUEST_TEMPLATE.md"));
    assert!(paths.contains(&"docs/Pull_Request_Template.markdown"));
    assert!(paths.contains(&".github/pull_request_template.txt"));
    assert!(paths.contains(&".github/PULL_REQUEST_TEMPLATE"));
    Ok(())
}

#[test]
fn contributor_verification_commands_cannot_drift_with_the_template() {
    let body = format!(
        "# Contributing\n\n## Verification and constrained environments\n\n```text\n{}\n```\n",
        pull_request::CONTRIBUTOR_VERIFICATION_COMMANDS.join("\n")
    );
    let canonical = crate::repository::markdown::parse_fragment(CONTRIBUTING, &body);
    assert!(pull_request::validate_contributor_fragment(&canonical).is_empty());

    let weakened = body.replace("cargo xtask repository", "cargo check --workspace");
    let document = crate::repository::markdown::parse_fragment(CONTRIBUTING, &weakened);
    let violations = pull_request::validate_contributor_fragment(&document);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("commands have drifted"));
}

fn canonical_pull_request_body() -> String {
    pull_request::canonical_source()
}

fn write_issue_policy(fixture: &Fixture) -> Result<(), std::io::Error> {
    fixture.write(
        CONTRIBUTING,
        &format!(
            "# Contributing\n\n## Issue timing and structure\n\n```markdown\n{}```\n",
            issue::canonical_issue_body()
        ),
    )
}

fn write_canonical_issue_templates(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    for path in [
        CAPABILITY_ISSUE_TEMPLATE,
        BUG_ISSUE_TEMPLATE,
        ISSUE_TEMPLATE_CONFIG,
    ] {
        let Some(expected) = issue::expected_form(path) else {
            return Err(format!("missing test contract for {path}").into());
        };
        fixture.write(path, &format!("{}\n", expected.join("\n")))?;
    }
    Ok(())
}

fn write_pull_request_policy(fixture: &Fixture) -> Result<(), std::io::Error> {
    fixture.write(
        CONTRIBUTING,
        &format!(
            "# Contributing\n\n## Verification and constrained environments\n\n```text\n{}\n```\n",
            pull_request::CONTRIBUTOR_VERIFICATION_COMMANDS.join("\n")
        ),
    )
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, std::io::Error> {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "edgeagent-template-check-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, relative: &str, contents: &str) -> Result<(), std::io::Error> {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _result = fs::remove_dir_all(&self.root);
    }
}
