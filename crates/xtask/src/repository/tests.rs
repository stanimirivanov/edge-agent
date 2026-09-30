use std::{collections::BTreeMap, error::Error, io, path::Path, path::PathBuf};

use super::*;

#[test]
fn current_repository_satisfies_policy() -> Result<(), Box<dyn Error>> {
    let report = check_at(Path::new(WORKSPACE_ROOT))?;
    if report.is_empty() {
        return Ok(());
    }

    let diagnostics = report
        .violations()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    Err(io::Error::other(format!(
        "checked-in repository policy must pass:\n{diagnostics}"
    ))
    .into())
}

#[test]
fn violation_diagnostics_identify_location_rule_fix_and_policy() -> Result<(), Box<dyn Error>> {
    let violation = Violation::new(
        "docs/example.md",
        7,
        "example.rule",
        "example violation",
        "correct the example",
        "CONTRIBUTING.md#example",
    );

    assert_eq!(
        violation.to_string(),
        "docs/example.md:7: [example.rule] example violation; fix: correct the example; \
         policy: CONTRIBUTING.md#example"
    );
    Ok(())
}

#[test]
fn checker_owned_policy_links_require_live_heading_anchors() -> Result<(), Box<dyn Error>> {
    let mut documents = BTreeMap::new();
    for (path, body) in [
        (
            "CONTRIBUTING.md",
            "# Contributing\n\n## Documentation\n\nPolicy.\n\n## Issue timing and structure\n\nPolicy.\n\n## Verification and constrained environments\n\nPolicy.\n\n## Pull request description\n\nPolicy.\n",
        ),
        (
            "docs/decisions/README.md",
            "# Decisions\n\n## Naming and lifecycle\n\nPolicy.\n",
        ),
        (
            "docs/roadmap/milestones.md",
            "# Milestones\n\n## Planning rules\n\nPolicy.\n",
        ),
        (
            "docs/development/harness.md",
            "# Harness\n\n## Repository policy\n\nPolicy.\n",
        ),
    ] {
        documents.insert(path.to_owned(), markdown::parse_fragment(path, body));
    }
    let mut repository = Repository {
        root: PathBuf::new(),
        documents,
    };
    assert!(check_policy_links(&repository).is_empty());

    repository.documents.insert(
        "docs/development/harness.md".to_owned(),
        markdown::parse_fragment("docs/development/harness.md", "# Harness\n"),
    );
    let violations = check_policy_links(&repository);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("#repository-policy"));
    Ok(())
}
