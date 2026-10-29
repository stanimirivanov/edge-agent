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
    "event-spine-processes",
    "nats-topology",
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
    "event-spine-producer",
    "event-spine-broker",
];
const OUTBOX_CHECKS: &[&str] = &[
    "event-spine-processes",
    "relay-observation-contracts",
    "relay-policy-contracts",
    "outbox-conformance",
    "outbox-fencing",
    "outbox-relay-boundaries",
    "outbox-recovery",
    "atomic-enqueue",
    "relay-conformance",
    "relay-recovery-budget",
    "event-spine-producer",
    "event-spine-client",
    "event-spine-broker",
];
const INBOX_CHECKS: &[&str] = &[
    "event-spine-processes",
    "event-spine-producer",
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
        matcher: Matcher::Prefix("services/gateway/"),
        guides: &["docs/development/gateway-admission.md"],
        checks: &["gateway-http", "gateway-admission", "sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Prefix("crates/research-postgres/"),
        guides: &["docs/development/research-persistence.md"],
        checks: &["research-persistence", "sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Exact("crates/research-application/src/publication.rs"),
        guides: &["docs/development/research-persistence.md"],
        checks: &["research-persistence"],
    },
    Route {
        matcher: Matcher::Prefix("crates/research-application/"),
        guides: &["docs/development/research-rendering.md"],
        checks: &["research-rendering"],
    },
    Route {
        matcher: Matcher::Exact("crates/research-domain/src/ranking.rs"),
        guides: &["docs/development/research-rendering.md"],
        checks: &["research-rendering"],
    },
    Route {
        matcher: Matcher::Exact("crates/research-domain/src/momentum.rs"),
        guides: &["docs/development/research-momentum.md"],
        checks: &["research-momentum"],
    },
    Route {
        matcher: Matcher::Exact("crates/research-domain/tests/momentum.rs"),
        guides: &["docs/development/research-momentum.md"],
        checks: &["research-momentum"],
    },
    Route {
        matcher: Matcher::Prefix("crates/research-domain/"),
        guides: &["docs/development/research-evidence.md"],
        checks: &["research-evidence"],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/src/research"),
        guides: &["docs/development/research-contracts.md"],
        checks: &["research-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/fixtures/research/"),
        guides: &["docs/development/research-contracts.md"],
        checks: &["research-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("services/market-data/src/offline_streaming/"),
        guides: &["docs/development/offline-stream-runtime.md"],
        checks: &[
            "offline-stream-runtime",
            "stream-supervision-contracts",
            "iex-canonical-acceptance",
        ],
    },
    Route {
        matcher: Matcher::Exact("services/market-data/src/lib.rs"),
        guides: &["docs/development/offline-stream-runtime.md"],
        checks: &["offline-stream-runtime"],
    },
    Route {
        matcher: Matcher::Exact("services/market-data/tests/offline_stream_runtime.rs"),
        guides: &["docs/development/offline-stream-runtime.md"],
        checks: &["offline-stream-runtime"],
    },
    Route {
        matcher: Matcher::Prefix("services/market-data/tests/stream_runtime/"),
        guides: &["docs/development/offline-stream-runtime.md"],
        checks: &["offline-stream-runtime"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-postgres/src/streaming/"),
        guides: &[
            "docs/development/offline-stream-runtime.md",
            "docs/development/sqlx-metadata.md",
        ],
        checks: &["offline-stream-runtime", "sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-http/src/iex_acceptance/replay.rs"),
        guides: &["docs/development/offline-stream-runtime.md"],
        checks: &[
            "offline-stream-runtime",
            "iex-canonical-acceptance",
            "provider-http-contracts",
        ],
    },
    Route {
        matcher: Matcher::Exact("docs/development/offline-stream-runtime.md"),
        guides: &["docs/development/offline-stream-runtime.md"],
        checks: &["offline-stream-runtime", "stream-supervision-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/streaming/publication/"),
        guides: &["docs/development/stream-native-provenance.md"],
        checks: &[
            "streaming-segment-contracts",
            "streaming-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact(
            "crates/market-data-application/tests/stream_segments/companion.rs",
        ),
        guides: &["docs/development/stream-native-provenance.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-object-store/tests/stream_segments/"),
        guides: &["docs/development/stream-native-provenance.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/contracts/src/evidence/stream_companion.rs"),
        guides: &["docs/development/stream-native-provenance.md"],
        checks: &[
            "streaming-segment-contracts",
            "iex-canonical-acceptance",
            "streaming-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-http/src/iex_acceptance/companion.rs"),
        guides: &["docs/development/stream-native-provenance.md"],
        checks: &["iex-canonical-acceptance", "provider-http-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-object-store/src/store/companion.rs"),
        guides: &["docs/development/stream-native-provenance.md"],
        checks: &[
            "streaming-segment-contracts",
            "streaming-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact(
            "crates/market-data-postgres/tests/stream_publication/companion.rs",
        ),
        guides: &["docs/development/stream-native-provenance.md"],
        checks: &["streaming-publication-postgres"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/stream-native-provenance.md"),
        guides: &["docs/development/stream-native-provenance.md"],
        checks: &[
            "streaming-segment-contracts",
            "iex-canonical-acceptance",
            "streaming-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/streaming/inputs.rs"),
        guides: &["docs/development/stream-owner-inputs.md"],
        checks: &[
            "streaming-segment-contracts",
            "streaming-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/tests/stream_segments/inputs.rs"),
        guides: &["docs/development/stream-owner-inputs.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-postgres/src/streaming/inputs.rs"),
        guides: &[
            "docs/development/stream-owner-inputs.md",
            "docs/development/sqlx-metadata.md",
        ],
        checks: &["streaming-publication-postgres", "sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-postgres/tests/stream_publication/"),
        guides: &["docs/development/stream-owner-inputs.md"],
        checks: &["streaming-publication-postgres"],
    },
    Route {
        matcher: Matcher::Exact(
            "crates/market-data-postgres/migrations/0010_stream_owner_inputs.sql",
        ),
        guides: &[
            "docs/development/stream-owner-inputs.md",
            "docs/development/sqlx-metadata.md",
        ],
        checks: &["streaming-publication-postgres", "sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-http/src/iex_acceptance/inputs.rs"),
        guides: &["docs/development/stream-owner-inputs.md"],
        checks: &["iex-canonical-acceptance", "provider-http-contracts"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/stream-owner-inputs.md"),
        guides: &["docs/development/stream-owner-inputs.md"],
        checks: &[
            "streaming-segment-contracts",
            "iex-canonical-acceptance",
            "streaming-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-http/src/iex_acceptance"),
        guides: &["docs/development/iex-canonical-acceptance.md"],
        checks: &["iex-canonical-acceptance", "alpaca-iex-conformance"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/iex-canonical-acceptance.md"),
        guides: &["docs/development/iex-canonical-acceptance.md"],
        checks: &["iex-canonical-acceptance"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/streaming/recovery.rs"),
        guides: &["docs/development/stream-supervision.md"],
        checks: &[
            "streaming-segment-contracts",
            "streaming-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-postgres/src/streaming/recovery.rs"),
        guides: &[
            "docs/development/stream-supervision.md",
            "docs/development/sqlx-metadata.md",
        ],
        checks: &["streaming-publication-postgres", "sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-postgres/migrations/0009_stream_recovery.sql"),
        guides: &[
            "docs/development/stream-supervision.md",
            "docs/development/sqlx-metadata.md",
        ],
        checks: &["streaming-publication-postgres", "sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/tests/stream_segments/"),
        guides: &["docs/development/stream-supervision.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/supervision"),
        guides: &["docs/development/stream-supervision.md"],
        checks: &["stream-supervision-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/tests/supervision.rs"),
        guides: &["docs/development/stream-supervision.md"],
        checks: &["stream-supervision-contracts"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/stream-supervision.md"),
        guides: &["docs/development/stream-supervision.md"],
        checks: &["stream-supervision-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-http/src/alpaca_iex"),
        guides: &["docs/development/alpaca-iex.md"],
        checks: &["alpaca-iex-conformance", "wss-transport-contracts"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/alpaca-iex.md"),
        guides: &["docs/development/alpaca-iex.md"],
        checks: &["alpaca-iex-conformance"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-http/src/wss"),
        guides: &["docs/development/streaming-transport.md"],
        checks: &["wss-transport-contracts"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/streaming-transport.md"),
        guides: &["docs/development/streaming-transport.md"],
        checks: &["wss-transport-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/evidence.rs"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-object-store/src/store.rs"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/src/evidence/stream"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/contracts/src/evidence.rs"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/streaming/segment"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/tests/stream_segments"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/tests/support/segment.rs"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-object-store/src/store/segment"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-object-store/tests/stream_segments.rs"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/storage.rs"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/streaming-segments.md"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-segment-contracts"],
    },
    Route {
        matcher: Matcher::Exact(
            "crates/market-data-postgres/migrations/0008_stream_publication.sql",
        ),
        guides: &["docs/development/streaming-segments.md"],
        checks: &[
            "streaming-segment-contracts",
            "streaming-publication-postgres",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-postgres/src/streaming/"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &[
            "streaming-segment-contracts",
            "streaming-publication-postgres",
            "streaming-session-postgres",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-postgres/tests/stream_publication.rs"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &["streaming-publication-postgres"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/streaming/publication"),
        guides: &["docs/development/streaming-segments.md"],
        checks: &[
            "streaming-segment-contracts",
            "streaming-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/streaming"),
        guides: &["docs/development/streaming-sessions.md"],
        checks: &["streaming-session-contracts", "streaming-session-postgres"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/tests/streaming"),
        guides: &["docs/development/streaming-sessions.md"],
        checks: &["streaming-session-contracts", "streaming-session-postgres"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/tests/support/streaming.rs"),
        guides: &["docs/development/streaming-sessions.md"],
        checks: &["streaming-session-contracts", "streaming-session-postgres"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-postgres/src/streaming"),
        guides: &["docs/development/streaming-sessions.md"],
        checks: &[
            "streaming-session-contracts",
            "streaming-session-postgres",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-postgres/tests/streaming.rs"),
        guides: &["docs/development/streaming-sessions.md"],
        checks: &[
            "streaming-session-contracts",
            "streaming-session-postgres",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Exact(
            "crates/market-data-postgres/migrations/0007_streaming_sessions.sql",
        ),
        guides: &["docs/development/streaming-sessions.md"],
        checks: &[
            "streaming-session-contracts",
            "streaming-session-postgres",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Exact("docs/development/streaming-sessions.md"),
        guides: &["docs/development/streaming-sessions.md"],
        checks: &["streaming-session-contracts", "streaming-session-postgres"],
    },
    Route {
        matcher: Matcher::Prefix("services/market-data/src/qualification/"),
        guides: &["docs/development/qualification-runtime.md"],
        checks: &[
            "qualification-owner-postgres",
            "qualification-preflight",
            "twelve-data-conformance",
            "alpaca-conformance",
            "native-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-postgres/"),
        guides: &["docs/development/qualification-owner.md"],
        checks: &["qualification-owner-postgres", "sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/qualification-owner.md"),
        guides: &["docs/development/qualification-owner.md"],
        checks: &["qualification-owner-postgres", "qualification-preflight"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/activation"),
        guides: &["docs/development/qualification-preflight.md"],
        checks: &["qualification-preflight"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/tests/activation"),
        guides: &["docs/development/qualification-preflight.md"],
        checks: &["qualification-preflight"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/qualification-preflight.md"),
        guides: &["docs/development/qualification-preflight.md"],
        checks: &["qualification-preflight"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/canonical.rs"),
        guides: &[
            "docs/development/qualification-preflight.md",
            "docs/development/native-daily-acceptance.md",
        ],
        checks: &[
            "qualification-preflight",
            "native-publication-contracts",
            "acquisition-terminal-contracts",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/budget/daily.rs"),
        guides: &["docs/development/qualification-preflight.md"],
        checks: &["qualification-preflight", "acquisition-budget-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/terminal"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &[
            "native-publication-contracts",
            "native-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/tests/native_publication"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &["native-publication-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/tests/support/native_store"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &[
            "native-publication-contracts",
            "native-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-postgres/"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &["native-publication-postgres"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-object-store/"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &[
            "native-publication-contracts",
            "native-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/retention.rs"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &[
            "native-publication-contracts",
            "native-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/storage.rs"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &[
            "native-publication-contracts",
            "native-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("docs/development/native-daily-acceptance.md"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &[
            "native-publication-contracts",
            "native-publication-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/evidence.rs"),
        guides: &[
            "docs/development/native-daily-acceptance.md",
            "docs/development/evidence-artifacts.md",
        ],
        checks: &["native-daily-acceptance", "evidence-freezing"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-domain/src/daily"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &["native-daily-acceptance", "market-data-quality"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/native_evidence"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &["native-daily-acceptance"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/tests/native_acceptance"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &["native-daily-acceptance"],
    },
    Route {
        matcher: Matcher::Exact("crates/contracts/src/evidence/native_daily.rs"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &["native-daily-acceptance", "evidence-contracts"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/native-daily-acceptance.md"),
        guides: &["docs/development/native-daily-acceptance.md"],
        checks: &["native-daily-acceptance", "evidence-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-domain/src/date_only.rs"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["date-only-evidence", "evidence-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/date_only.rs"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["date-only-evidence", "evidence-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/tests/date_only.rs"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["date-only-evidence", "evidence-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("services/market-data/"),
        guides: &["docs/development/offline-worker.md"],
        checks: &[
            "offline-worker-contracts",
            "offline-worker-processes",
            "offline-worker-recovery",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-replay/"),
        guides: &[
            "docs/development/offline-market-data.md",
            "docs/development/instrument-selection.md",
        ],
        checks: &[
            "offline-source-isolation",
            "offline-source-network-denial",
            "instrument-selection-routing",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/selection"),
        guides: &["docs/development/instrument-selection.md"],
        checks: &["instrument-selection-routing", "offline-source-isolation"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/instrument-selection.md"),
        guides: &["docs/development/instrument-selection.md"],
        checks: &["instrument-selection-routing", "offline-source-isolation"],
    },
    Route {
        matcher: Matcher::Exact("scripts/verify_offline_source.py"),
        guides: &["docs/development/offline-market-data.md"],
        checks: &["offline-source-isolation", "offline-source-network-denial"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/offline-market-data.md"),
        guides: &["docs/development/offline-market-data.md"],
        checks: &["offline-source-isolation", "offline-source-network-denial"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/terminal"),
        guides: &["docs/development/acquisition-publication.md"],
        checks: &[
            "acquisition-terminal-contracts",
            "acquisition-terminal-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/tests/terminal.rs"),
        guides: &["docs/development/acquisition-publication.md"],
        checks: &[
            "acquisition-terminal-contracts",
            "acquisition-terminal-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("docs/development/acquisition-publication.md"),
        guides: &["docs/development/acquisition-publication.md"],
        checks: &[
            "acquisition-terminal-contracts",
            "acquisition-terminal-postgres",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-postgres/"),
        guides: &[
            "docs/development/acquisition-publication.md",
            "docs/development/acquisition-ledger.md",
            "docs/development/sqlx-metadata.md",
        ],
        checks: &[
            "acquisition-terminal-contracts",
            "acquisition-terminal-postgres",
            "acquisition-budget-contracts",
            "acquisition-ledger-postgres",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/admission"),
        guides: &["docs/development/acquisition-ledger.md"],
        checks: &[
            "acquisition-budget-contracts",
            "acquisition-ledger-postgres",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/budget"),
        guides: &["docs/development/acquisition-ledger.md"],
        checks: &[
            "acquisition-budget-contracts",
            "acquisition-ledger-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/tests/admission.rs"),
        guides: &["docs/development/acquisition-ledger.md"],
        checks: &[
            "acquisition-budget-contracts",
            "acquisition-ledger-postgres",
        ],
    },
    Route {
        matcher: Matcher::Exact("docs/development/acquisition-ledger.md"),
        guides: &["docs/development/acquisition-ledger.md"],
        checks: &[
            "acquisition-budget-contracts",
            "acquisition-ledger-postgres",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-object-store/"),
        guides: &["docs/development/evidence-storage.md"],
        checks: &["evidence-storage-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/retention.rs"),
        guides: &["docs/development/evidence-retention.md"],
        checks: &[
            "evidence-retention-contracts",
            "evidence-retention-postgres",
            "evidence-storage-contracts",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Exact("docs/development/evidence-retention.md"),
        guides: &["docs/development/evidence-retention.md"],
        checks: &[
            "evidence-retention-contracts",
            "evidence-retention-postgres",
            "evidence-storage-contracts",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/tests/retention.rs"),
        guides: &["docs/development/evidence-retention.md"],
        checks: &["evidence-retention-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-postgres/src/retention"),
        guides: &["docs/development/evidence-retention.md"],
        checks: &[
            "evidence-retention-contracts",
            "evidence-retention-postgres",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-postgres/tests/retention.rs"),
        guides: &["docs/development/evidence-retention.md"],
        checks: &["evidence-retention-postgres"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/storage.rs"),
        guides: &["docs/development/evidence-storage.md"],
        checks: &["evidence-storage-contracts"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/evidence-storage.md"),
        guides: &["docs/development/evidence-storage.md"],
        checks: &["evidence-storage-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-http/"),
        guides: &[
            "docs/development/provider-http.md",
            "docs/development/market-data-acquisition.md",
        ],
        checks: &[
            "provider-http-contracts",
            "metered-provider-dispatch",
            "twelve-data-conformance",
            "alpaca-conformance",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/src/precise_daily"),
        guides: &["docs/development/alpaca.md"],
        checks: &["alpaca-conformance", "market-data-conformance"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/alpaca.md"),
        guides: &["docs/development/alpaca.md"],
        checks: &["alpaca-conformance"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/date_only_source.rs"),
        guides: &[
            "docs/development/twelve-data.md",
            "docs/development/native-daily-acceptance.md",
        ],
        checks: &[
            "native-daily-acceptance",
            "twelve-data-conformance",
            "date-only-evidence",
            "evidence-contracts",
        ],
    },
    Route {
        matcher: Matcher::Exact("docs/development/twelve-data.md"),
        guides: &["docs/development/twelve-data.md"],
        checks: &["twelve-data-conformance"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/provider-http.md"),
        guides: &["docs/development/provider-http.md"],
        checks: &["provider-http-contracts", "metered-provider-dispatch"],
    },
    Route {
        matcher: Matcher::Exact("crates/market-data-application/src/dispatch.rs"),
        guides: &[
            "docs/development/provider-http.md",
            "docs/development/acquisition-ledger.md",
        ],
        checks: &["metered-provider-dispatch"],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/src/evidence"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &[
            "evidence-contracts",
            "evidence-freezing",
            "date-only-evidence",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/schemas/evidence/"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["evidence-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/fixtures/evidence-v1/"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["evidence-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/fixtures/evidence-date-only-v1/"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["date-only-evidence", "evidence-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/event-spine-conformance/src/evidence.rs"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["evidence-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/event-spine-conformance/examples/evidence_schemas.rs"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["evidence-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/event-spine-conformance/tests/evidence_contracts.rs"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["evidence-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/event-spine-conformance/fixtures/evidence-baselines.txt"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["evidence-contracts"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/evidence-artifacts.md"),
        guides: &["docs/development/evidence-artifacts.md"],
        checks: &["evidence-contracts", "evidence-freezing"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-application/"),
        guides: &[
            "docs/development/market-data-acquisition.md",
            "docs/development/evidence-artifacts.md",
        ],
        checks: &["market-data-conformance", "evidence-freezing"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/market-data-acquisition.md"),
        guides: &["docs/development/market-data-acquisition.md"],
        checks: &["market-data-conformance"],
    },
    Route {
        matcher: Matcher::Prefix("crates/market-data-domain/"),
        guides: &["docs/development/market-data.md"],
        checks: &["market-data-quality"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/market-data.md"),
        guides: &["docs/development/market-data.md"],
        checks: &["market-data-quality"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/m02-qualification.md"),
        guides: &["docs/development/m02-qualification.md"],
        checks: &[
            "event-spine-processes",
            "nats-topology",
            "message-schema-contracts",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/messaging-nats/src/topology"),
        guides: &["docs/development/jetstream-topology.md"],
        checks: &["nats-topology"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/jetstream-topology.md"),
        guides: &["docs/development/jetstream-topology.md"],
        checks: &["nats-topology"],
    },
    Route {
        matcher: Matcher::Prefix("crates/event-spine-conformance/"),
        guides: &[
            "docs/development/message-contracts.md",
            "docs/development/plans/m02-completion.md",
            "docs/development/process-acceptance.md",
            "docs/development/m02-qualification.md",
            "docs/development/event-spine-telemetry.md",
            "docs/development/quarantine-replay.md",
        ],
        checks: &[
            "message-schema-contracts",
            "nats-topology",
            "event-spine-processes",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/"),
        guides: &[],
        checks: &[
            "message-schema-contracts",
            "nats-topology",
            "event-spine-processes",
        ],
    },
    Route {
        matcher: Matcher::Exact("docs/development/quarantine-replay.md"),
        guides: &["docs/development/quarantine-replay.md"],
        checks: &["event-spine-processes", "nats-topology", "sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/event-spine-telemetry.md"),
        guides: &["docs/development/event-spine-telemetry.md"],
        checks: &["event-spine-processes", "relay-observation-contracts"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/process-acceptance.md"),
        guides: &["docs/development/process-acceptance.md"],
        checks: &["event-spine-processes"],
    },
    Route {
        matcher: Matcher::Exact("scripts/verify_sqlx_query_policy.py"),
        guides: &["docs/development/sqlx-metadata.md"],
        checks: &[],
    },
    Route {
        matcher: Matcher::Exact("scripts/qualify_daily.py"),
        guides: &["docs/development/qualification-harness.md"],
        checks: &["harness-tests"],
    },
    Route {
        matcher: Matcher::Exact("tests/test_qualify_daily.py"),
        guides: &["docs/development/qualification-harness.md"],
        checks: &["harness-tests"],
    },
    Route {
        matcher: Matcher::Exact("docs/development/qualification-harness.md"),
        guides: &["docs/development/qualification-harness.md"],
        checks: &["harness-tests"],
    },
    Route {
        matcher: Matcher::Exact("tests/test_verify_sqlx_query_policy.py"),
        guides: &["docs/development/sqlx-metadata.md"],
        checks: &[],
    },
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
        matcher: Matcher::Prefix("crates/audit-domain/"),
        guides: &[
            "docs/development/account-audit.md",
            "docs/development/frozen-evaluation.md",
        ],
        checks: &[
            "account-audit-contracts",
            "frozen-evaluation-contracts",
            "account-admission-processes",
        ],
    },
    Route {
        matcher: Matcher::Exact("crates/contracts/src/audit.rs"),
        guides: &["docs/development/account-audit.md"],
        checks: &["account-audit-contracts", "account-admission-processes"],
    },
    Route {
        matcher: Matcher::Prefix("crates/simulation-postgres/src/audit"),
        guides: &[
            "docs/development/account-audit.md",
            "docs/development/sqlx-metadata.md",
        ],
        checks: &[
            "account-audit-contracts",
            "account-admission-processes",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/simulation-domain/src/analytics"),
        guides: &["docs/development/option-analytics.md"],
        checks: &["option-analytics-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/simulation-domain/tests/analytics"),
        guides: &["docs/development/option-analytics.md"],
        checks: &["option-analytics-contracts"],
    },
    Route {
        matcher: Matcher::Exact("scripts/generate_option_analytics_references.py"),
        guides: &["docs/development/option-analytics.md"],
        checks: &["option-analytics-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/simulation-postgres/"),
        guides: &[
            "docs/development/account-admission.md",
            "docs/development/evidence-execution.md",
            "docs/development/sqlx-metadata.md",
        ],
        checks: &["account-admission-processes", "sqlx-metadata"],
    },
    Route {
        matcher: Matcher::Prefix("crates/simulation-postgres/src/settlement"),
        guides: &["docs/development/option-settlement.md"],
        checks: &[
            "account-admission-processes",
            "simulation-schema-contracts",
            "sqlx-metadata",
        ],
    },
    Route {
        matcher: Matcher::Prefix("crates/simulation-domain/src/settlement"),
        guides: &["docs/development/option-settlement.md"],
        checks: &["account-admission-processes", "simulation-schema-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/contracts/src/settlement.rs"),
        guides: &["docs/development/option-settlement.md"],
        checks: &["account-admission-processes", "simulation-schema-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/schemas/simulation/settle"),
        guides: &["docs/development/option-settlement.md"],
        checks: &["account-admission-processes", "simulation-schema-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/event-spine-conformance/src/accounts/"),
        guides: &["docs/development/evidence-execution.md"],
        checks: &["account-admission-processes", "simulation-schema-contracts"],
    },
    Route {
        matcher: Matcher::Prefix("crates/simulation-domain/src/execution"),
        guides: &[
            "docs/development/evidence-execution.md",
            "docs/development/dry-run-orders.md",
        ],
        checks: &["account-admission-processes", "simulation-schema-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/contracts/src/execution.rs"),
        guides: &["docs/development/evidence-execution.md"],
        checks: &["account-admission-processes", "simulation-schema-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/event-spine-conformance/tests/account_admission.rs"),
        guides: &["docs/development/account-admission.md"],
        checks: &["account-admission-processes"],
    },
    Route {
        matcher: Matcher::Prefix("crates/event-spine-conformance/tests/accounts/"),
        guides: &["docs/development/account-admission.md"],
        checks: &["account-admission-processes"],
    },
    Route {
        matcher: Matcher::Exact("crates/event-spine-conformance/src/accounts.rs"),
        guides: &["docs/development/account-admission.md"],
        checks: &["account-admission-processes", "simulation-schema-contracts"],
    },
    Route {
        matcher: Matcher::Exact("crates/event-spine-conformance/src/bin/account-worker.rs"),
        guides: &["docs/development/account-admission.md"],
        checks: &["account-admission-processes"],
    },
    Route {
        matcher: Matcher::Prefix("crates/contracts/schemas/simulation/"),
        guides: &["docs/development/account-admission.md"],
        checks: &["simulation-schema-contracts", "account-admission-processes"],
    },
    Route {
        matcher: Matcher::Exact("crates/contracts/src/simulation.rs"),
        guides: &["docs/development/account-admission.md"],
        checks: &["simulation-schema-contracts", "account-admission-processes"],
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
        checks: &[
            "event-spine-client",
            "event-spine-broker",
            "event-spine-producer",
        ],
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
            "docs/development/event-spine-telemetry.md",
            "docs/development/local-platform.md",
            "docs/development/event-spine-conformance.md",
        ],
        checks: OUTBOX_CHECKS,
    },
    Route {
        matcher: Matcher::Prefix("crates/telemetry/"),
        guides: &["docs/development/event-spine-telemetry.md"],
        checks: &[
            "event-spine-client",
            "relay-observation-contracts",
            "event-spine-processes",
        ],
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
    // containment is lexical so foreign and POSIX paths receive the same validation.
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
