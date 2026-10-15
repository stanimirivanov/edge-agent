//! Advisory task routing. Changed paths never remove default acceptance checks.

use std::collections::BTreeSet;

use super::{DEFAULT_COMMAND, inventory, workspace_root};

#[cfg(test)]
mod tests;

const BASE_GUIDES: &[&str] = &["docs/README.md", "docs/development/harness.md"];
const PLATFORM_GUIDES: &[&str] = &[
    "docs/development/local-platform.md",
    "docs/development/event-spine-conformance.md",
];
const NATS_CHECKS: &[&str] = &[
    "nats-publish",
    "nats-consume",
    "nats-abandoned-settlement",
    "nats-oversize",
    "nats-no-stream",
    "nats-size-rejection",
    "nats-header-only",
    "nats-publish-oversize",
    "nats-settlement-response",
    "nats-halted-worker",
    "nats-closed-intake",
    "nats-intake-bound",
    "nats-fixture-cleanup",
    "event-spine-client",
    "event-spine-broker",
];
const OUTBOX_CHECKS: &[&str] = &[
    "outbox-conformance",
    "outbox-fencing",
    "outbox-relay-boundaries",
    "atomic-enqueue",
    "relay-conformance",
    "event-spine-client",
    "event-spine-broker",
];
const INBOX_CHECKS: &[&str] = &[
    "inbox-invariants",
    "inbox-errors",
    "handler-ordering",
    "atomic-enqueue",
    "event-spine-client",
    "event-spine-broker",
];
const SUPPLY_CHECKS: &[&str] = &[
    "dependency-policy",
    "sbom-rust",
    "sbom-images",
    "sbom-completeness",
];
const ALL_ADVISORY_CHECKS: &[&str] = &[
    "dependency-policy",
    "m08-event-spine-dependency-policy",
    "m08-duroxide-dependency-policy",
    "m08-dbos-dependency-policy",
    "m08-temporal-dependency-policy",
    "m08-flawless-dependency-policy",
];
const EVENT_SPINE_PROOF_CHECKS: &[&str] = &[
    "m08-event-spine-dependency-policy",
    "m08-event-spine-format",
    "m08-event-spine-clippy",
    "m08-event-spine-test",
];

#[derive(Clone, Copy, Debug)]
enum Matcher {
    Exact(&'static str),
    Prefix(&'static str),
    WorkspaceCargo,
    EventSpineDependency,
}

impl Matcher {
    fn matches(self, path: &str) -> bool {
        match self {
            Self::Exact(expected) => path == expected,
            Self::Prefix(prefix) => path.starts_with(prefix),
            Self::WorkspaceCargo => {
                !path.starts_with("experiments/")
                    && matches!(path.rsplit('/').next(), Some("Cargo.toml" | "Cargo.lock"))
            }
            Self::EventSpineDependency => {
                path == "Cargo.toml"
                    || [
                        "crates/contracts/",
                        "crates/inbox-handler/",
                        "crates/inbox-postgres/",
                        "crates/messaging/",
                        "crates/messaging-nats/",
                        "crates/telemetry/",
                    ]
                    .iter()
                    .any(|prefix| path.starts_with(prefix))
            }
        }
    }
}

struct Route {
    matcher: Matcher,
    guides: &'static [&'static str],
    checks: &'static [&'static str],
}

// This table points to canonical policy; it does not restate crate roles or
// dependency allowances. Overlapping routes form a union, never an override.
const ROUTES: &[Route] = &[
    Route {
        matcher: Matcher::Exact(".github/dependabot.yml"),
        guides: &["docs/development/supply-chain.md"],
        checks: ALL_ADVISORY_CHECKS,
    },
    Route {
        matcher: Matcher::Exact(".github/workflows/dependency-audit.yml"),
        guides: &["docs/development/supply-chain.md"],
        checks: ALL_ADVISORY_CHECKS,
    },
    Route {
        matcher: Matcher::Exact("experiments/deny.toml"),
        guides: &["docs/development/supply-chain.md"],
        checks: ALL_ADVISORY_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix(".github/workflows/"),
        guides: &["docs/development/ci-security.md"],
        checks: &["secret-scanner-contracts", "secret-scan"],
    },
    Route {
        matcher: Matcher::Exact(".gitleaks.toml"),
        guides: &["docs/development/ci-security.md"],
        checks: &["secret-scanner-contracts", "secret-scan"],
    },
    Route {
        matcher: Matcher::Exact("supply-chain/security-tools.toml"),
        guides: &["docs/development/ci-security.md"],
        checks: &["secret-scanner-contracts", "secret-scan"],
    },
    Route {
        matcher: Matcher::Exact("scripts/verify_secrets.py"),
        guides: &["docs/development/ci-security.md"],
        checks: &["secret-scanner-contracts", "secret-scan"],
    },
    Route {
        matcher: Matcher::Exact("scripts/install_gitleaks.py"),
        guides: &["docs/development/ci-security.md"],
        checks: &["secret-scanner-contracts", "secret-scan"],
    },
    Route {
        matcher: Matcher::Prefix("crates/"),
        guides: &["docs/development/engineering-standards.md"],
        checks: &[],
    },
    Route {
        matcher: Matcher::Prefix("services/"),
        guides: &[
            "docs/development/engineering-standards.md",
            "docs/architecture/system-overview.md",
        ],
        checks: &["image-smoke"],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/"),
        guides: &[
            "docs/development/message-contracts.md",
            "docs/development/event-spine-conformance.md",
        ],
        checks: &["event-spine-client", "event-spine-broker"],
    },
    Route {
        matcher: Matcher::Prefix("crates/messaging/"),
        guides: &["docs/development/messaging-adapters.md"],
        checks: NATS_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("crates/messaging/"),
        guides: PLATFORM_GUIDES,
        checks: OUTBOX_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("crates/messaging/"),
        guides: &[],
        checks: INBOX_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("crates/messaging-nats/"),
        guides: &[
            "docs/development/messaging-adapters.md",
            "docs/development/local-platform.md",
            "docs/development/event-spine-conformance.md",
        ],
        checks: NATS_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("crates/inbox-handler/"),
        guides: &[
            "docs/development/inbox-handler.md",
            "docs/development/event-spine-conformance.md",
            "docs/development/local-platform.md",
        ],
        checks: INBOX_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("crates/inbox-postgres/"),
        guides: &[
            "docs/development/postgres-inbox.md",
            "docs/development/sqlx-metadata.md",
            "docs/development/local-platform.md",
            "docs/development/event-spine-conformance.md",
        ],
        checks: INBOX_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("crates/inbox-postgres/"),
        guides: &[],
        checks: &["sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Prefix("crates/outbox-postgres/"),
        guides: &[
            "docs/development/postgres-outbox.md",
            "docs/development/sqlx-metadata.md",
            "docs/development/local-platform.md",
            "docs/development/event-spine-conformance.md",
        ],
        checks: OUTBOX_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("crates/outbox-postgres/"),
        guides: &[],
        checks: &["sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Prefix("crates/outbox-relay/"),
        guides: &[
            "docs/development/outbox-relay.md",
            "docs/development/local-platform.md",
            "docs/development/event-spine-conformance.md",
        ],
        checks: OUTBOX_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("crates/telemetry/"),
        guides: &["docs/development/event-spine-telemetry.md"],
        checks: &["event-spine-client"],
    },
    Route {
        matcher: Matcher::Prefix(".sqlx/"),
        guides: &["docs/development/sqlx-metadata.md"],
        checks: &["sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Exact("Dockerfile"),
        guides: &["docs/development/supply-chain.md"],
        checks: &["image-smoke"],
    },
    Route {
        matcher: Matcher::Exact(".dockerignore"),
        guides: &["docs/development/supply-chain.md"],
        checks: &["image-smoke"],
    },
    Route {
        matcher: Matcher::Exact("deploy/images.toml"),
        guides: &["docs/development/supply-chain.md"],
        checks: &["image-smoke"],
    },
    Route {
        matcher: Matcher::Prefix("deploy/"),
        guides: &["docs/architecture/deployment-portability.md"],
        checks: &["image-smoke"],
    },
    Route {
        matcher: Matcher::Prefix("deploy/local/"),
        guides: PLATFORM_GUIDES,
        checks: NATS_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("deploy/local/"),
        guides: &[],
        checks: OUTBOX_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("deploy/local/"),
        guides: &[],
        checks: INBOX_CHECKS,
    },
    Route {
        matcher: Matcher::WorkspaceCargo,
        guides: &[
            "docs/architecture/dependency-rules.md",
            "docs/development/supply-chain.md",
        ],
        checks: SUPPLY_CHECKS,
    },
    Route {
        matcher: Matcher::Exact("deny.toml"),
        guides: &["docs/development/supply-chain.md"],
        checks: SUPPLY_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("supply-chain/"),
        guides: &[
            "docs/development/supply-chain.md",
            "docs/development/releases.md",
        ],
        checks: SUPPLY_CHECKS,
    },
    Route {
        matcher: Matcher::Exact("Dockerfile"),
        guides: &[],
        checks: SUPPLY_CHECKS,
    },
    Route {
        matcher: Matcher::Exact("deploy/images.toml"),
        guides: &[],
        checks: SUPPLY_CHECKS,
    },
    Route {
        matcher: Matcher::Exact("scripts/generate_sboms.py"),
        guides: &["docs/development/supply-chain.md"],
        checks: SUPPLY_CHECKS,
    },
    Route {
        matcher: Matcher::Exact(".github/workflows/release-images.yml"),
        guides: &["docs/development/releases.md"],
        checks: SUPPLY_CHECKS,
    },
    Route {
        matcher: Matcher::EventSpineDependency,
        guides: &["experiments/README.md"],
        checks: EVENT_SPINE_PROOF_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("experiments/"),
        guides: &["experiments/README.md"],
        checks: &[],
    },
    Route {
        matcher: Matcher::Prefix("experiments/event-spine-proof/"),
        guides: &["experiments/README.md"],
        checks: EVENT_SPINE_PROOF_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("experiments/durable-execution/"),
        guides: &["experiments/durable-execution/README.md"],
        checks: &[
            "m08-duroxide-dependency-policy",
            "m08-duroxide-format",
            "m08-duroxide-clippy",
            "m08-duroxide-test",
        ],
    },
    Route {
        matcher: Matcher::Prefix("experiments/dbos-proof/"),
        guides: &["experiments/dbos-proof/README.md"],
        checks: &[
            "m08-dbos-dependency-policy",
            "m08-dbos-format",
            "m08-dbos-clippy",
            "m08-dbos-test",
        ],
    },
    Route {
        matcher: Matcher::Prefix("experiments/temporal-proof/"),
        guides: &["experiments/temporal-proof/README.md"],
        checks: &[
            "m08-temporal-dependency-policy",
            "m08-temporal-format",
            "m08-temporal-clippy",
            "m08-temporal-test",
        ],
    },
    Route {
        matcher: Matcher::Prefix("experiments/flawless-proof/"),
        guides: &["experiments/flawless-proof/README.md"],
        checks: &[
            "m08-flawless-dependency-policy",
            "m08-flawless-format",
            "m08-flawless-clippy",
            "m08-flawless-test",
        ],
    },
];

#[derive(Debug, Eq, PartialEq)]
struct Advice {
    mandatory: &'static str,
    guides: BTreeSet<&'static str>,
    checks: BTreeSet<&'static str>,
}

pub(super) fn print(paths: &[String]) -> Result<(), String> {
    validate()?;
    let advice = advise(paths)?;
    println!(
        "Mandatory default acceptance (never path-filtered): {}",
        advice.mandatory
    );
    println!("Read canonical guidance:");
    for guide in advice.guides {
        println!("  {guide}");
    }
    println!("Specialized checks (advisory; prerequisites and evidence: cargo xtask checks):");
    for check in advice.checks {
        println!("  cargo xtask verify --check {check}");
    }
    println!(
        "Routing cannot infer every behavioral impact; review applicability and report omissions."
    );
    Ok(())
}

pub(super) fn validate() -> Result<(), String> {
    let ids = inventory::checks()
        .into_iter()
        .map(|check| check.id)
        .collect();
    validate_references(&ids, |guide| workspace_root().join(guide).is_file())
}

fn validate_references(
    available_ids: &BTreeSet<&str>,
    guide_exists: impl Fn(&str) -> bool,
) -> Result<(), String> {
    for guide in BASE_GUIDES
        .iter()
        .copied()
        .chain(ROUTES.iter().flat_map(|route| route.guides.iter().copied()))
    {
        if !guide_exists(guide) {
            return Err(format!(
                "routing guide missing: {guide}; update the canonical route"
            ));
        }
    }
    for check in ROUTES.iter().flat_map(|route| route.checks.iter().copied()) {
        if !available_ids.contains(check) {
            return Err(format!(
                "routing check missing from canonical inventory: {check}"
            ));
        }
    }
    Ok(())
}

fn advise(paths: &[String]) -> Result<Advice, String> {
    let mut advice = Advice {
        mandatory: DEFAULT_COMMAND,
        guides: BASE_GUIDES.iter().copied().collect(),
        checks: BTreeSet::new(),
    };
    for path in paths {
        let path = normalize(path)?;
        for route in ROUTES.iter().filter(|route| route.matcher.matches(&path)) {
            advice.guides.extend(route.guides.iter().copied());
            advice.checks.extend(route.checks.iter().copied());
        }
    }
    Ok(advice)
}

fn normalize(path: &str) -> Result<String, String> {
    let normalized = path.replace('\\', "/");
    // Deleted paths remain routable. No input path is opened or executed, and
    // containment is lexical so Windows and POSIX reject the same inputs.
    if normalized.is_empty()
        || normalized
            .chars()
            .any(|character| character.is_control() || character == ':')
        || normalized
            .split('/')
            .any(|component| matches!(component, "" | "." | ".."))
    {
        return Err("changed paths must be nonempty Git-relative paths without absolute, drive, dot or traversal components".to_owned());
    }
    Ok(normalized)
}
