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

const PUBLISHED_ADRS: [PublishedIdentity; 33] = [
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
    PublishedIdentity {
        number: 11,
        path: "docs/decisions/0011-bound-untrusted-envelopes-before-decoding.md",
        title: "Bound untrusted envelopes before decoding",
    },
    PublishedIdentity {
        number: 12,
        path: "docs/decisions/0012-keep-message-contract-diagnostics-payload-safe.md",
        title: "Keep message contract diagnostics payload-safe",
    },
    PublishedIdentity {
        number: 13,
        path: "docs/decisions/0013-select-durable-workflow-execution.md",
        title: "Select Temporal for durable workflow execution",
    },
    PublishedIdentity {
        number: 14,
        path: "docs/decisions/0014-use-sqlx-for-postgresql-inbox.md",
        title: "Use SQLx for the PostgreSQL inbox adapter",
    },
    PublishedIdentity {
        number: 15,
        path: "docs/decisions/0015-allow-atomic-sqlx-outbox-enqueue.md",
        title: "Allow atomic SQLx outbox enqueue",
    },
    PublishedIdentity {
        number: 16,
        path: "docs/decisions/0016-use-sqlx-throughout-the-postgresql-outbox.md",
        title: "Use SQLx throughout the PostgreSQL outbox",
    },
    PublishedIdentity {
        number: 17,
        path: "docs/decisions/0017-align-delivery-and-quarantine-bounds.md",
        title: "Align delivery and quarantine bounds",
    },
    PublishedIdentity {
        number: 18,
        path: "docs/decisions/0018-use-validated-message-failure-codes.md",
        title: "Use validated message failure codes",
    },
    PublishedIdentity {
        number: 19,
        path: "docs/decisions/0019-validate-outbox-timing-at-the-port.md",
        title: "Validate outbox timing at the port",
    },
    PublishedIdentity {
        number: 20,
        path: "docs/decisions/0020-retain-settlement-abandonment-diagnostics.md",
        title: "Retain settlement abandonment diagnostics",
    },
    PublishedIdentity {
        number: 21,
        path: "docs/decisions/0021-enforce-ci-security-and-secret-scanning.md",
        title: "Enforce CI security and secret scanning",
    },
    PublishedIdentity {
        number: 22,
        path: "docs/decisions/0022-bound-relay-publication-by-acquired-claims.md",
        title: "Bound relay publication by acquired claims",
    },
    PublishedIdentity {
        number: 23,
        path: "docs/decisions/0023-type-relay-configuration-and-runtime-failures.md",
        title: "Type relay configuration and runtime failures",
    },
    PublishedIdentity {
        number: 24,
        path: "docs/decisions/0024-define-synthetic-event-spine-acceptance-contracts.md",
        title: "Define synthetic event-spine acceptance contracts",
    },
    PublishedIdentity {
        number: 25,
        path: "docs/decisions/0025-derive-bounded-jetstream-topology-from-the-registry.md",
        title: "Derive bounded JetStream topology from the registry",
    },
    PublishedIdentity {
        number: 26,
        path: "docs/decisions/0026-compose-durable-acceptance-in-independent-processes.md",
        title: "Compose durable acceptance in independent processes",
    },
    PublishedIdentity {
        number: 27,
        path: "docs/decisions/0027-enforce-service-owned-semantic-command-ordering.md",
        title: "Enforce service-owned semantic command ordering",
    },
    PublishedIdentity {
        number: 28,
        path: "docs/decisions/0028-compose-authorized-recoverable-quarantine-replay.md",
        title: "Compose authorized recoverable quarantine replay",
    },
    PublishedIdentity {
        number: 29,
        path: "docs/decisions/0029-export-bounded-correlated-event-spine-observations.md",
        title: "Export bounded correlated event-spine observations",
    },
    PublishedIdentity {
        number: 30,
        path: "docs/decisions/0030-qualify-event-spine-failure-recovery.md",
        title: "Qualify event-spine failure recovery",
    },
    PublishedIdentity {
        number: 31,
        path: "docs/decisions/0031-define-the-supported-market-evidence-profile.md",
        title: "Define the supported market evidence profile",
    },
    PublishedIdentity {
        number: 32,
        path: "docs/decisions/0032-define-vendor-neutral-simulation-profiles.md",
        title: "Define vendor-neutral simulation profiles",
    },
    PublishedIdentity {
        number: 33,
        path: "docs/decisions/0033-freeze-deterministic-run-continuations.md",
        title: "Freeze deterministic run continuations",
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
