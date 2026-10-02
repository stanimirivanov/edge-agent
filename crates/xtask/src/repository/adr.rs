mod index;
mod lifecycle;
mod record;
mod supersession;
mod template;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use super::{ADR_POLICY, Repository, Violation};

const ADR_DIRECTORY: &str = "docs/decisions/";
const ADR_INDEX: &str = "docs/decisions/README.md";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PublishedIdentity {
    number: usize,
    path: &'static str,
    title: &'static str,
}

const PUBLISHED_ADRS: [PublishedIdentity; 10] = [
    PublishedIdentity {
        number: 1,
        path: "docs/decisions/0001-use-a-rust-workspace-with-multiple-deployables.md",
        title: "Use a Rust workspace with multiple deployables",
    },
    PublishedIdentity {
        number: 2,
        path: "docs/decisions/0002-use-portable-at-least-once-event-messaging.md",
        title: "Use portable at-least-once event messaging",
    },
    PublishedIdentity {
        number: 3,
        path: "docs/decisions/0003-add-dry-run-trade-execution.md",
        title: "Add dry-run trade execution",
    },
    PublishedIdentity {
        number: 4,
        path: "docs/decisions/0004-separate-application-ui-gitops-and-substrate-ownership.md",
        title: "Separate application, UI, GitOps, and substrate ownership",
    },
    PublishedIdentity {
        number: 5,
        path: "docs/decisions/0005-standardize-message-routing-and-retention.md",
        title: "Standardize message routing and retention",
    },
    PublishedIdentity {
        number: 6,
        path: "docs/decisions/0006-keep-inbound-coordination-persistence-neutral.md",
        title: "Keep inbound coordination persistence-neutral",
    },
    PublishedIdentity {
        number: 7,
        path: "docs/decisions/0007-enforce-workspace-capability-dependencies.md",
        title: "Enforce workspace capability dependencies",
    },
    PublishedIdentity {
        number: 8,
        path: "docs/decisions/0008-enforce-progressive-coding-harness-policy.md",
        title: "Enforce progressive coding-harness policy",
    },
    PublishedIdentity {
        number: 9,
        path: "docs/decisions/0009-fence-outbox-transitions-by-claim-generation.md",
        title: "Fence outbox transitions by claim generation",
    },
    PublishedIdentity {
        number: 10,
        path: "docs/decisions/0010-keep-delivery-metadata-transport-neutral.md",
        title: "Keep delivery metadata transport-neutral",
    },
];

const LATEST_PUBLISHED_ADR: usize = PUBLISHED_ADRS.len();

const REQUIRED_SECTIONS: [&str; 8] = [
    "TL;DR",
    "Context",
    "Decision",
    "Alternatives considered",
    "Consequences",
    "Compatibility and migration",
    "Security and operations",
    "Validation",
];

const METADATA_FIELDS: [&str; 6] = [
    "Status",
    "Date",
    "Milestone",
    "Deciders",
    "Supersedes",
    "Superseded by",
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct Record {
    number: usize,
    path: String,
    status: Option<String>,
    status_line: usize,
    milestone: Option<String>,
    milestone_line: usize,
    supersedes: Vec<usize>,
    supersedes_line: usize,
    superseded_by: Option<usize>,
    superseded_by_line: usize,
}

impl Record {
    fn new(number: usize, path: String) -> Self {
        Self {
            number,
            path,
            status: None,
            status_line: 1,
            milestone: None,
            milestone_line: 1,
            supersedes: Vec::new(),
            supersedes_line: 1,
            superseded_by: None,
            superseded_by_line: 1,
        }
    }
}

pub(super) fn check(repository: &Repository) -> Vec<Violation> {
    let (records, mut violations) = record::read(repository);
    violations.extend(template::validate(repository));
    violations.extend(record::validate_milestones(repository, &records));
    violations.extend(validate_sequence(&records));
    violations.extend(index::validate(repository, &records));
    violations.extend(supersession::validate(&records));
    violations
}

fn validate_sequence(records: &BTreeMap<usize, Record>) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (index, (number, record)) in records.iter().enumerate() {
        let expected = index + 1;
        if *number != expected {
            violations.push(adr_violation(
                &record.path,
                1,
                "adr.sequence",
                format!("ADR sequence expected {expected:04} before {number:04}"),
                "restore the missing historical decision or renumber only an unpublished record and its references",
            ));
            break;
        }
    }

    let latest = records.keys().next_back().copied().unwrap_or_default();
    if latest != LATEST_PUBLISHED_ADR {
        violations.push(adr_violation(
            ADR_INDEX,
            1,
            "adr.sequence",
            format!(
                "latest ADR is {latest:04}; repository policy records {LATEST_PUBLISHED_ADR:04}"
            ),
            "restore deleted history, or advance `LATEST_PUBLISHED_ADR` only while adding and indexing the next decision",
        ));
    }
    violations
}

fn adr_violation(
    path: impl Into<String>,
    line: usize,
    rule: &'static str,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation::new(path, line, rule, message, fix, ADR_POLICY)
}
