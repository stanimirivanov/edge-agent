//! One ordered inventory for local and CI acceptance; selectors cannot edit argv.

use super::{Check, CommandSpec, Discovery, Program, Requirement, proofs};

pub(super) const PROFILES: &[&str] = &[
    "verify",
    "offline-ingestion",
    "fmt",
    "repository",
    "architecture",
    "check",
    "test",
    "local-platform",
    "m02-qualification",
    "image-smoke",
    "supply-chain",
    "dependency-audit",
    "sqlx-metadata",
    "ci-security",
    "m08-event-spine-proof",
    "m08-durable-execution",
    "m08-dbos-proof",
    "m08-temporal-proof",
    "m08-flawless-proof",
    "m08-event-spine-proof-static",
    "m08-durable-execution-static",
    "m08-dbos-proof-static",
    "m08-temporal-proof-static",
    "m08-flawless-proof-static",
    "m08-event-spine-proof-test",
    "m08-durable-execution-test",
    "m08-dbos-proof-test",
    "m08-temporal-proof-test",
    "m08-flawless-proof-test",
];

fn check(
    id: &'static str,
    tier: &'static str,
    profiles: &'static [&'static str],
    program: Program,
    args: &[&'static str],
    evidence: &'static str,
) -> Check {
    Check {
        id,
        tier,
        profiles,
        command: CommandSpec {
            program,
            args: args.to_vec(),
        },
        prerequisites: &[],
        discovery: None,
        evidence,
    }
}

pub(super) fn checks() -> Vec<Check> {
    use Program::{Cargo, Python, Xtask};
    let mut checks = vec![
        check(
            "text-format",
            "T2",
            &["verify", "fmt"],
            Python,
            &["scripts/verify_repository.py", "--format-check"],
            "UTF-8, LF and final-newline contract; not prose correctness.",
        ),
        check(
            "rust-format",
            "T2",
            &["verify", "fmt"],
            Cargo,
            &["fmt", "--all", "--check"],
            "Pinned rustfmt output; not architecture or runtime behavior.",
        ),
        check(
            "repository-bytes",
            "T1",
            &["verify", "check", "repository"],
            Python,
            &["scripts/verify_repository.py"],
            "Required repository inventory and independent byte/document baseline.",
        ),
        check(
            "repository-policy",
            "T1",
            &["verify", "check", "repository"],
            Xtask,
            &["repository"],
            "Rendered local navigation, governance and canonical entrypoint parity; not factual freshness.",
        ),
        check(
            "architecture-declarations",
            "T1",
            &["verify", "check", "architecture"],
            Python,
            &["scripts/verify_architecture.py"],
            "All direct manifest declarations, including inactive target and optional dependencies.",
        ),
        check(
            "workflow-security",
            "T1",
            &[
                "verify",
                "check",
                "architecture",
                "ci-security",
                "dependency-audit",
            ],
            Xtask,
            &["workflow-security"],
            "Parsed CI action, trigger, credential and permission policy; not remote runner isolation or branch protection.",
        ),
        check(
            "secret-policy",
            "T1",
            &["verify", "check", "repository", "ci-security"],
            Python,
            &["scripts/verify_secrets.py", "--policy-only"],
            "Pinned scanner and exact fixture exception configuration; not an executed credential scan.",
        ),
        check(
            "architecture-graph",
            "T1",
            &["verify", "check", "architecture"],
            Xtask,
            &["architecture"],
            "Cargo-normalized internal edges and capability policy; not semantic cohesion.",
        ),
        check(
            "image-contracts",
            "T2",
            &["verify", "check"],
            Python,
            &["scripts/verify_images.py"],
            "Static image build/runtime declarations, not a successful image build.",
        ),
        check(
            "local-stack-contracts",
            "T2",
            &["verify", "check"],
            Python,
            &["scripts/verify_local_stack.py"],
            "Static portable local-profile contracts, not running service readiness.",
        ),
        check(
            "supply-chain-contracts",
            "T2",
            &["verify", "check", "dependency-audit"],
            Python,
            &["scripts/verify_supply_chain.py"],
            "Declared dependency/SBOM policy shape; not fresh advisories or generated SBOM completeness.",
        ),
        check(
            "release-contracts",
            "T2",
            &["verify", "check"],
            Python,
            &["scripts/verify_release.py"],
            "Static release contract shape; not signatures of published artifacts.",
        ),
        check(
            "locked-graph",
            "T2",
            &["verify", "check"],
            Cargo,
            &[
                "metadata",
                "--locked",
                "--offline",
                "--format-version",
                "1",
                "--no-deps",
            ],
            "Root workspace locked metadata resolves without network; not isolated experiment locks.",
        ),
        check(
            "rust-check",
            "T2",
            &["verify", "check"],
            Cargo,
            &["check", "--locked", "--workspace", "--all-targets"],
            "Every root workspace target compiles with committed SQLx offline metadata.",
        ),
        check(
            "rust-clippy",
            "T2",
            &["verify", "check"],
            Cargo,
            &[
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            "Workspace lint policy with warnings rejected.",
        ),
        check(
            "sqlx-query-policy",
            "T2",
            &["verify", "check"],
            Python,
            &["scripts/verify_sqlx_query_policy.py"],
            "Real offline Clippy fixtures accept checked SQL and reject outbox runtime/unchecked query APIs, including aliases.",
        ),
        check(
            "harness-tests",
            "T2",
            &["verify", "test"],
            Python,
            &[
                "-m",
                "unittest",
                "discover",
                "-s",
                "tests",
                "-p",
                "test_*.py",
            ],
            "Deterministic Python mutation and script tests.",
        ),
        check(
            "ignored-inventory",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "--workspace",
                "--all-targets",
                "--",
                "--ignored",
                "--list",
            ],
            "Exact ignored-test census; neither discovery nor compilation executes service proofs.",
        ),
        check(
            "rust-tests",
            "T2",
            &["verify", "test"],
            Cargo,
            &["test", "--locked", "--workspace", "--all-targets"],
            "Credential-free unit/integration contracts; ignored service tests remain unproved.",
        ),
        check(
            "rust-doctests",
            "T2",
            &["verify", "test"],
            Cargo,
            &["test", "--locked", "--workspace", "--doc"],
            "Public API positive and compile-fail examples, absent from --all-targets.",
        ),
        check(
            "option-analytics-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-simulation-domain",
                "--test",
                "analytics",
                "--",
            ],
            "Independent numerical references, finite differences, point-in-time boundaries and bounded IV convergence; not provider or policy qualification.",
        ),
        check(
            "account-audit-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-audit-domain",
                "--test",
                "replay",
                "--",
            ],
            "Bounded owner-export replay, checkpoint/backfill equivalence, duplicates/conflicts, financial reconciliation and scope; not public deployment or risk authority.",
        ),
        check(
            "frozen-evaluation-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-audit-domain",
                "--test",
                "evaluation",
                "--",
            ],
            "Synthetic frozen-run replay and one-factor fee/latency/liquidity sensitivity; no empirical market or statistical performance claim.",
        ),
        check(
            "streaming-segment-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "-p",
                "edgeagent-market-data-object-store",
                "--test",
                "stream_segments",
                "--",
            ],
            "Offline bounded normalized capture and scoped immutable storage; not checkpoints, readiness or vendor qualification.",
        ),
        check(
            "wss-transport-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-http",
                "--lib",
                "wss::tests::",
                "--",
            ],
            "Synthetic TLS loopback and controlled-clock transport proofs; no vendor activation or source readiness.",
        ),
        check(
            "stream-supervision-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "supervision",
                "--",
            ],
            "Offline bounded FIFO/fair scheduling, unresolved ownership and sticky failures; not durable recovery or evidence readiness.",
        ),
        check(
            "iex-canonical-acceptance",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-http",
                "--lib",
                "iex_acceptance::tests::",
                "--features",
                "serde_json/preserve_order,compression-unification-test",
                "--",
            ],
            "Invented owner-pinned IEX conditions, full references, source identities and native provenance; not runtime activation or readiness.",
        ),
        check(
            "alpaca-iex-conformance",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-http",
                "--lib",
                "alpaca_iex::tests::",
                "--features",
                "serde_json/preserve_order,compression-unification-test",
                "--",
            ],
            "Invented Alpaca IEX protocol and local TLS proofs only; no vendor activation, continuity or readiness.",
        ),
        check(
            "streaming-session-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "streaming",
                "--",
            ],
            "Finite explicit selection/authority, session ceilings, sticky accounting and scoped generations; no provider I/O.",
        ),
        check(
            "offline-worker-contracts",
            "T2",
            &["verify", "test", "offline-ingestion"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data",
                "--bin",
                "edgeagent-market-data",
                "--",
            ],
            "Offline-only composition configuration, pinned replay and non-inferred cleanup authority; process recovery is separate.",
        ),
        check(
            "offline-source-isolation",
            "T2",
            &["verify", "test", "offline-ingestion"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-replay",
                "--test",
                "offline",
                "--",
            ],
            "Network-free invented daily source, ambient credential isolation and P05 prefix replay; OS network denial is a separate Linux CI proof.",
        ),
        check(
            "instrument-selection-routing",
            "T2",
            &["verify", "test", "offline-ingestion"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-replay",
                "--test",
                "selection",
                "--",
            ],
            "Invented header-first routing, immutable selection/directory history, narrowed requests and fail-closed control/sequence/framing proofs; not native feed qualification.",
        ),
        check(
            "offline-source-network-denial",
            "T3",
            &["offline-ingestion"],
            Python,
            &["scripts/verify_offline_source.py"],
            "Linux fresh network namespace executes the complete synthetic source suite without external routes; requires sudo/unshare and a populated Cargo cache.",
        ),
        check(
            "market-data-quality",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-domain",
                "--test",
                "daily",
                "--",
            ],
            "Bounded point-in-time daily-series quality against explicit calendars, buckets and action coverage; no acquisition or license authority.",
        ),
        check(
            "market-data-conformance",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "contracts",
                "--",
            ],
            "Synthetic acquisition admission, reviewed rights/capabilities and response conformance; no live qualification or legal authority.",
        ),
        check(
            "acquisition-terminal-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "terminal",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Receipt-first recovery, uncertain confirmation and authority/storage prerequisites; model proofs, not SQL atomicity.",
        ),
        check(
            "acquisition-budget-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "admission",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Canonical admission and accounting: original deadlines, conservative uncertainty, replay and account rate fences. Synthetic model, not SQL atomicity.",
        ),
        check(
            "evidence-retention-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "retention",
                "--",
            ],
            "Durable denial surrounds I/O; cleanup requires explicit write quiescence and preserves inventory after uncertain deletion. Synthetic model, not physical media erasure.",
        ),
        check(
            "evidence-storage-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-object-store",
                "--lib",
                "--",
            ],
            "Synthetic create-only storage, uncertain-write recovery, scoped authority, bounded integrity reads, local reopen and scoped idempotent deletion; not cloud or media erasure.",
        ),
        check(
            "provider-http-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-http",
                "--lib",
                "--features",
                "serde_json/preserve_order,compression-unification-test",
                "--",
                "--skip",
                "tests::metered::",
                "--skip",
                "tests::twelve_data::",
                "--skip",
                "tests::alpaca::",
            ],
            "Loopback-only exact JSON and bounded HTTP authority, streaming, cancellation, deadline, status and TLS-failure proofs; not vendor qualification.",
        ),
        check(
            "metered-provider-dispatch",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-http",
                "--lib",
                "tests::metered::",
                "--features",
                "serde_json/preserve_order,compression-unification-test",
                "--",
            ],
            "Loopback HTTP and committed-state model prove permit-before-I/O, conservative uncertainty, idempotent page confirmation and safe usage; not process/PostgreSQL qualification.",
        ),
        check(
            "evidence-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-event-spine-conformance",
                "--test",
                "evidence_contracts",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Immutable evidence schemas, canonical bytes and scoped terminal contracts, including preserved JSON object order.",
        ),
        check(
            "evidence-freezing",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "evidence",
                "--",
            ],
            "Checked authority and whole-series quality bridge to immutable scoped evidence; no I/O or publication.",
        ),
        check(
            "native-daily-acceptance",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "native_acceptance",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Complete native-date quality, original provenance, current authority, owner lineage and independent canonical accepted format; no storage/publication or provider calls.",
        ),
        check(
            "native-publication-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "native_publication",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Native preparation recovery, exact frozen identity, authorized predecessor reads and current durable denial; model proof, not PostgreSQL or live activation.",
        ),
        check(
            "qualification-preflight",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "activation",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Offline exact-profile owner review, current authority, account uncertainty and non-resetting credits. Never enables dispatch or qualifies a live provider.",
        ),
        check(
            "date-only-evidence",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "date_only",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Native labels, reviewed DST/core coverage, precise local receipt and bounded canonical provenance. No source timestamp conversion, provider I/O or publication activation.",
        ),
        check(
            "twelve-data-conformance",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-http",
                "--lib",
                "tests::twelve_data::",
                "--",
            ],
            "Invented loopback daily protocol, exact native-date capture, authority/credit fencing and accounting-only confirmation. No provider calls or credentials.",
        ),
        check(
            "messaging-error-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-messaging",
                "--test",
                "error_redaction",
                "--",
            ],
            "Explicit payload-safe public error target retained from CI, with a protected named-test census.",
        ),
        check(
            "alpaca-conformance",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-http",
                "--lib",
                "tests::alpaca::",
                "--features",
                "serde_json/preserve_order,compression-unification-test",
                "--",
            ],
            "Invented historical SIP pages preserve precise source times and exact values; prove permit-bound paging, confirmation recovery, current authority and no feed fallback. No provider calls.",
        ),
        check(
            "relay-observation-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-outbox-relay",
                "--test",
                "telemetry",
                "--",
            ],
            "Isolated relay telemetry trust boundary and confirmed outcome matrix; named discovery rejects missing or renamed security regressions.",
        ),
        check(
            "relay-policy-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-outbox-relay",
                "--test",
                "policy_contracts",
                "--",
            ],
            "Typed startup bounds/precision and independent runtime category/source contracts; protected named discovery is credential-free.",
        ),
        check(
            "contracts-preserve-order",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-contracts",
                "--features",
                "serde_json/preserve_order",
            ],
            "Canonical persisted envelope bytes survive serde_json feature unification.",
        ),
        check(
            "gateway-http",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-gateway",
                "--test",
                "http",
                "--",
            ],
            "Bounded local HTTP validation, caller binding and acknowledgement timing; no database required.",
        ),
        check(
            "gateway-query-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-gateway",
                "--test",
                "query",
                "--",
            ],
            "Scoped Gateway/projector HTTP queries, exact outcomes, observation states, upstream bounds and failure refusal; no database required.",
        ),
        check(
            "projector-query-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-audit-projector",
                "--test",
                "contracts",
                "--",
            ],
            "Internal query scope/Host/Origin/write refusal before storage, local configuration, descriptor compatibility and safe diagnostics.",
        ),
        check(
            "research-lifecycle",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-research-domain",
                "--test",
                "lifecycle",
                "--",
            ],
            "Explicit-time irreversible lifecycle state/revision policy without storage or a scheduler.",
        ),
        check(
            "research-projection-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-audit-domain",
                "--test",
                "research",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Pure scoped Research replay, immutable outcomes, lifecycle gap refusal and anchored bounded checkpoint integrity under map-order feature unification.",
        ),
        check(
            "research-worker-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-research",
                "--test",
                "contracts",
                "--",
            ],
            "Deterministic owner-byte conversion, digest rejection, local configuration and descriptor compatibility.",
        ),
        check(
            "research-rendering",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-research-application",
                "--test",
                "rendering",
                "--features",
                "map-order-test",
                "--",
            ],
            "Research request binding, deterministic ranking, canonical artifact/abstention fixtures and refusal paths; no runtime or publication claim.",
        ),
        check(
            "research-momentum",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-research-domain",
                "--test",
                "momentum",
                "--",
            ],
            "Raw USD endpoint return, exact ties-to-even ppm rounding, half-ppm error and range checks; no strategy approval or ranking.",
        ),
        check(
            "research-evidence",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-research-domain",
                "--test",
                "evidence",
                "--",
            ],
            "Pure research minimum-evidence policy, point-in-time boundaries and order-independent abstention; no authentication or I/O claim.",
        ),
        check(
            "research-acquisition-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-contracts",
                "--test",
                "research_acquisition",
                "--features",
                "schema,serde_json/preserve_order",
                "--",
            ],
            "Owner research evidence route/correlation, strict DTO refusal and frozen schema freshness.",
        ),
        check(
            "research-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-contracts",
                "--test",
                "research",
                "--features",
                "schema,serde_json/preserve_order",
                "--",
            ],
            "Research v1 canonical fixtures, structural schema freshness and fail-closed validation; no runtime activation.",
        ),
        check(
            "message-schema-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-event-spine-conformance",
                "--test",
                "schema_contracts",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Generated local schema freshness, frozen baselines, typed payload/redaction and conservative optional-addition contracts; no broker/process-recovery claim.",
        ),
        check(
            "image-smoke",
            "T3",
            &["image-smoke"],
            Python,
            &["scripts/verify_images.py", "--build"],
            "OCI builds and non-root/read-only/networkless smoke tests, not cloud deployment.",
        ),
        check(
            "simulation-schema-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-event-spine-conformance",
                "--test",
                "simulation_contracts",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Protected synthetic admission constructors, schemas, mode rejection and immutable published v1 identities; no provider qualification.",
        ),
        check(
            "sqlx-metadata",
            "T3",
            &["sqlx-metadata"],
            Xtask,
            &["sqlx-metadata"],
            "Regenerated query metadata matches all service-owned migration directories; requires fresh isolated local Compose PostgreSQL and pinned SQLx CLI, not runtime atomicity.",
        ),
        check(
            "dependency-policy",
            "T3",
            &["supply-chain", "dependency-audit"],
            Cargo,
            &["deny", "--locked", "check"],
            "Current advisory/license/source/bans policy; requires network and cargo-deny.",
        ),
        check(
            "sbom-rust",
            "T3",
            &["supply-chain"],
            Python,
            &[
                "scripts/generate_sboms.py",
                "rust",
                "--output",
                "artifacts/sbom",
            ],
            "Generated Rust binary SBOMs with pinned cargo-cyclonedx.",
        ),
        check(
            "sbom-images",
            "T3",
            &["supply-chain"],
            Python,
            &[
                "scripts/generate_sboms.py",
                "images",
                "--output",
                "artifacts/sbom",
            ],
            "Generated image SBOMs with pinned Syft and Docker.",
        ),
        check(
            "sbom-completeness",
            "T3",
            &["supply-chain"],
            Python,
            &[
                "scripts/verify_supply_chain.py",
                "--artifacts",
                "artifacts/sbom",
            ],
            "Complete generated artifact set matches declared deployables; not release attestation.",
        ),
    ];
    checks.push(check(
        "secret-scanner-contracts",
        "T3",
        &["ci-security"],
        Python,
        &["scripts/verify_secrets.py", "--self-test"],
        "Actual pinned scanner rejects synthetic credentials and proves path-and-value fixture exceptions; not repository cleanliness.",
    ));
    checks.push(check(
        "secret-scan",
        "T3",
        &["ci-security"],
        Python,
        &["scripts/verify_secrets.py"],
        "Pinned redacted scan of source working tree and complete reachable Git history; not privacy or licensed-data certification.",
    ));
    for entry in &mut checks {
        match entry.id {
            "stream-supervision-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    ignored: false,
                    expected: vec![
                        "supervision_round_robin_preserves_fifo_and_bounds_every_eligible_wait",
                        "supervision_busy_and_failed_sources_cannot_starve_other_lanes",
                        "supervision_overload_retains_rejected_and_unresolved_records_without_eviction",
                        "supervision_foreign_and_dropped_dispatches_never_confirm_or_retry_heads",
                        "supervision_cancellation_and_source_failure_are_sticky_without_compensation",
                        "supervision_configuration_binding_and_diagnostics_fail_closed",
                    ],
                });
            }
            "iex-canonical-acceptance" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    ignored: false,
                    expected: vec![
                        "iex_acceptance::tests::iex_acceptance_trade_identity_replay_and_native_provenance_are_exact",
                        "iex_acceptance::tests::iex_owner_inputs_derive_exact_namespace_claims_from_the_complete_profile",
                        "iex_acceptance::tests::iex_acceptance_minute_bars_preserve_raw_coverage_count_vwap_and_bounds",
                        "iex_acceptance::tests::iex_acceptance_rejects_unknown_semantics_stale_references_and_authority",
                        "iex_acceptance::tests::iex_acceptance_profile_bytes_bind_complete_inputs_and_reject_ambiguity",
                    ],
                });
            }
            "alpaca-iex-conformance" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    ignored: false,
                    expected: vec![
                        "alpaca_iex::tests::iex_protocol_requires_exact_handshake_and_subscription",
                        "alpaca_iex::tests::iex_exact_trades_and_minute_bars_preserve_scope_time_and_values",
                        "alpaca_iex::tests::iex_batches_reject_ambiguity_corrections_and_partial_success",
                        "alpaca_iex::tests::iex_known_unselected_payload_skips_values_but_not_control_or_mapping_checks",
                        "alpaca_iex::tests::iex_loopback_tls_auth_subscription_fragments_and_controls_are_charged",
                        "alpaca_iex::tests::iex_unknown_accounting_revocation_and_cancellation_stop_without_reconnect",
                        "alpaca_iex::tests::iex_abandoned_intake_and_absolute_handshake_deadline_are_terminal",
                        "alpaca_iex::tests::iex_configuration_and_diagnostics_do_not_expand_scope_or_expose_data",
                    ],
                });
            }
            "wss-transport-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    ignored: false,
                    expected: vec![
                        "wss::tests::wss_charges_controls_fragments_and_exact_caps_before_exposure",
                        "wss::tests::wss_rejects_untrusted_tls_bad_hostname_and_invalid_upgrades",
                        "wss::tests::wss_protocol_overflow_and_lost_charge_are_terminal",
                        "wss::tests::wss_cancellation_denial_and_abandoned_receive_never_reuse_socket",
                        "wss::tests::wss_guard_bounds_idle_partial_work_and_total_lifetime",
                        "wss::tests::wss_configuration_and_diagnostics_fail_closed",
                        "wss::tests::wss_abandoned_committed_charge_never_exposes_or_replays_payload",
                        "wss::tests::wss_fragment_and_outgoing_budgets_are_not_renewed_by_progress",
                        "wss::tests::wss_frame_ceiling_is_inclusive_and_silent_upgrade_times_out",
                    ],
                });
            }
            "streaming-segment-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "stream_segment_freezing_binds_session_scope_epoch_generation_and_retention",
                        "stream_segment_codec_rejects_noncanonical_corrupt_and_unbounded_input",
                        "stream_segment_values_preserve_cancellations_and_source_semantics",
                        "stream_publication_recovery_preserves_exact_bytes_and_opaque_event",
                        "recovery::stream_restore_replays_original_intent_and_rechecks_after_effects",
                        "recovery::stream_restore_empty_cancelled_or_stale_recovery_never_opens_barrier",
                        "inputs::stream_owner_inputs_bind_complete_bytes_and_reject_ambiguous_or_stale_records",
                        "companion::native_companion_binds_exact_records_owner_inputs_and_legacy_recovery",
                        "stream_retained_authority_outlives_capture_not_original_rights",
                        "stream_deletion_requires_quiescence_and_preserves_other_namespaces",
                        "stream_segment_store_is_create_only_scoped_and_reopens",
                        "stream_segment_store_recovers_uncertain_put_and_denies_revocation_expiry",
                        "stream_segment_store_rejects_descriptor_and_content_attacks",
                        "companion::native_companion_store_verifies_uncertain_creation_denial_and_paired_cleanup",
                    ],
                    ignored: false,
                });
            }
            "streaming-session-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "stream_admission_pins_complete_selection_and_current_authority",
                        "stream_windows_limits_and_delivery_never_inherit_daily_defaults",
                        "stream_frame_receipts_charge_controls_and_stop_at_exact_limits",
                        "stream_overages_and_invalid_state_cannot_refund_or_continue",
                        "stream_connection_fences_bind_session_account_and_preserve_start_snapshot",
                    ],
                    ignored: false,
                });
            }
            "offline-source-network-denial" => {
                entry.prerequisites = &[
                    Requirement::Executable("unshare"),
                    Requirement::Executable("sudo"),
                ];
            }
            "account-audit-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "complete_rebuild_checkpoint_resume_and_reordered_backfill_agree",
                        "duplicates_conflicts_and_foreign_scope_never_mutate_a_view",
                        "financial_drift_and_revision_regression_fail_atomically",
                        "capacity_corruption_truncation_and_unknown_versions_fail_closed",
                        "invalid_wire_state_ordering_reservations_and_nullable_presence_are_rejected",
                        "immutable_export_identity_and_payload_safe_diagnostics_are_stable",
                    ],
                    ignored: false,
                });
            }
            "offline-source-isolation" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "offline_mode_is_default_and_live_or_malformed_flags_fail_closed",
                        "daily_source_returns_meaningful_repeatable_canonical_batches",
                        "replay_restores_every_prefix_without_repeating_ids_or_random_draws",
                        "fixture_bytes_scope_bounds_and_provenance_are_checked_before_replay",
                        "cancellation_and_foreign_admission_leave_the_checkpoint_unchanged",
                        "ambient_credentials_cannot_activate_live_access",
                    ],
                    ignored: false,
                });
            }
            "instrument-selection-routing" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "latest_known_revisions_never_resurrect_obsolete_selection_or_directories",
                        "narrowed_requests_fail_before_provider_invocation_and_keep_exact_aliases",
                        "framing_and_header_selection_run_before_full_payload_decoding",
                        "controls_gaps_unknown_keys_and_stale_epochs_cannot_hide_behind_filtering",
                        "canonical_binding_tracks_content_revisions_and_order_without_diagnostic_leaks",
                        "bounds_ambiguous_mappings_and_malformed_frames_fail_closed",
                    ],
                    ignored: false,
                });
            }
            "acquisition-terminal-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "committed_recovery_skips_preparation_and_preserves_original_event_bytes",
                        "failed_or_cancelled_confirmation_recovers_without_compensation",
                        "ready_preparation_requires_matching_admission_storage_and_current_authority",
                        "ready_guards_pending_cancelled_and_expired_work_while_rejection_retains_charges",
                        "receipts_bind_exact_route_command_identity_and_payload_safe_diagnostics",
                    ],
                    ignored: false,
                });
            }
            "offline-worker-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "offline::tests::offline_mode_rejects_vendor_and_live_flags_before_infrastructure",
                        "offline::tests::compiled_profile_is_pinned_and_cancellation_cannot_activate_a_source",
                        "offline::tests::history_recipes_preserve_legacy_identity_and_replay_exact_batches",
                        "offline::tests::history_quality_accepts_complete_and_refuses_missing_or_action_mismatch",
                        "offline::tests::cleanup_never_infers_account_or_write_quiescence",
                        "offline::tests::profile_lock_excludes_a_second_owner_and_refuses_reinitialization",
                        "offline::tests::compiled_profile_passes_quality_and_creates_a_scoped_artifact",
                        "qualification::tests::both_complete_owner_inputs_round_trip_and_reject_changed_or_unknown_values",
                        "qualification::tests::owner_input_errors_redact_payload_and_profile_requires_explicit_arguments",
                        "qualification::tests::precise_preparation_rehydrates_original_time_and_passes_quality_again",
                    ],
                    ignored: false,
                });
            }
            "acquisition-budget-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "admission_canonical_digest_binds_complete_inputs_and_current_authority",
                        "configuration_v2_roundtrips_without_reinterpreting_legacy_admissions",
                        "admission_decode_rejects_noncanonical_unknown_duplicate_and_tampered_records",
                        "attempt_call_page_and_byte_budgets_survive_state_recovery",
                        "account_limits_spacing_window_and_pending_permits_span_commands",
                        "cancellation_deadlines_and_uncertain_calls_do_not_refund_or_release",
                        "completion_replay_is_immutable_and_old_fences_cannot_release_new_work",
                        "daily_policy_bootstrap_and_legacy_accounts_fail_closed_without_reset",
                        "daily_credits_roll_at_utc_midnight_without_releasing_pending_or_refunding",
                        "daily_unknown_prices_expiry_clock_regression_and_overflow_deny_without_mutation",
                    ],
                    ignored: false,
                });
            }
            "evidence-retention-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "durable_denial_checks_surround_storage_and_never_release_revoked_bytes",
                        "cleanup_requires_quiescence_and_keeps_inventory_after_uncertain_deletion",
                    ],
                    ignored: false,
                });
            }
            "evidence-storage-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "store::tests::concurrent_creates_and_corrections_preserve_exact_immutable_bytes",
                        "store::tests::lost_write_completion_and_dropped_operations_recover_without_overwrite",
                        "store::tests::corrupt_collisions_descriptors_and_stream_bounds_fail_closed",
                        "store::tests::current_authority_and_exclusive_expiry_are_rechecked_after_io",
                        "store::tests::durable_local_store_reopens_and_licensed_namespaces_do_not_alias",
                        "store::tests::storage_errors_and_adapter_debug_redact_paths_hashes_and_payloads",
                        "store::tests::quiescent_deletion_is_scoped_idempotent_and_recovers_after_timeout",
                        "store::tests::native::native_uncertain_create_recovers_exact_bytes_and_rejects_other_formats",
                        "store::tests::native::native_reads_recheck_authority_expiry_scope_and_quiescent_cleanup",
                    ],
                    ignored: false,
                });
            }
            "provider-http-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "tests::exact_json_tokens_retain_precision_and_reject_ambiguous_or_excess_values",
                        "tests::endpoint_authority_credentials_and_redirects_cannot_expand_egress",
                        "tests::response_streams_and_local_attempt_budgets_fail_closed_at_boundaries",
                        "tests::cancellation_original_deadlines_and_dropped_attempts_do_not_restart_work",
                        "tests::statuses_retry_after_encodings_and_public_diagnostics_are_explicit",
                        "tests::tls_handshake_failure_never_downgrades_or_exposes_request_secrets",
                        "alpaca_iex::tests::iex_protocol_requires_exact_handshake_and_subscription",
                        "alpaca_iex::tests::iex_exact_trades_and_minute_bars_preserve_scope_time_and_values",
                        "alpaca_iex::tests::iex_batches_reject_ambiguity_corrections_and_partial_success",
                        "alpaca_iex::tests::iex_known_unselected_payload_skips_values_but_not_control_or_mapping_checks",
                        "alpaca_iex::tests::iex_loopback_tls_auth_subscription_fragments_and_controls_are_charged",
                        "alpaca_iex::tests::iex_unknown_accounting_revocation_and_cancellation_stop_without_reconnect",
                        "alpaca_iex::tests::iex_abandoned_intake_and_absolute_handshake_deadline_are_terminal",
                        "alpaca_iex::tests::iex_configuration_and_diagnostics_do_not_expand_scope_or_expose_data",
                        "iex_acceptance::tests::iex_owner_inputs_derive_exact_namespace_claims_from_the_complete_profile",
                        "iex_acceptance::tests::iex_acceptance_trade_identity_replay_and_native_provenance_are_exact",
                        "iex_acceptance::tests::iex_acceptance_minute_bars_preserve_raw_coverage_count_vwap_and_bounds",
                        "iex_acceptance::tests::iex_acceptance_rejects_unknown_semantics_stale_references_and_authority",
                        "iex_acceptance::tests::iex_acceptance_profile_bytes_bind_complete_inputs_and_reject_ambiguity",
                        "wss::tests::wss_charges_controls_fragments_and_exact_caps_before_exposure",
                        "wss::tests::wss_rejects_untrusted_tls_bad_hostname_and_invalid_upgrades",
                        "wss::tests::wss_protocol_overflow_and_lost_charge_are_terminal",
                        "wss::tests::wss_cancellation_denial_and_abandoned_receive_never_reuse_socket",
                        "wss::tests::wss_guard_bounds_idle_partial_work_and_total_lifetime",
                        "wss::tests::wss_configuration_and_diagnostics_fail_closed",
                        "wss::tests::wss_abandoned_committed_charge_never_exposes_or_replays_payload",
                        "wss::tests::wss_fragment_and_outgoing_budgets_are_not_renewed_by_progress",
                        "wss::tests::wss_frame_ceiling_is_inclusive_and_silent_upgrade_times_out",
                    ],
                    ignored: false,
                });
            }
            "metered-provider-dispatch" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "tests::metered::each_page_requires_confirmed_permit_and_usage_is_ledger_backed",
                        "tests::metered::lost_reservation_confirmation_and_inflight_drop_never_redispatch",
                        "tests::metered::completed_page_reconfirmation_survives_lost_or_dropped_confirmation",
                        "tests::metered::authority_cancellation_and_original_deadlines_are_rechecked_after_waits",
                        "tests::metered::partial_overflow_and_retry_after_accounting_stays_conservative",
                        "tests::metered::dispatch_diagnostics_and_usage_exclude_identity_and_payloads",
                    ],
                    ignored: false,
                });
            }
            "evidence-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "generated_evidence_artifacts_have_exact_inventory_and_immutable_baselines",
                        "canonical_evidence_bytes_preserve_exact_values_times_and_scoped_identity",
                        "malformed_units_knowledge_coverage_and_lineage_cannot_form_artifacts",
                        "bounded_canonical_decode_rejects_tampering_duplicates_and_payload_diagnostics",
                        "evidence_routes_bind_schema_owner_partition_and_reference_only_outcomes",
                        "local_schemas_reject_version_drift_unknown_fields_and_missing_explicit_nulls",
                    ],
                    ignored: false,
                });
            }
            "qualification-preflight" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "qualification_profiles_are_bounded_canonical_strict_and_payload_safe",
                        "qualification_preflight_covers_both_profiles_without_enabling_or_mutating_execution",
                        "qualification_preflight_requires_independent_current_owner_opt_in",
                        "qualification_preflight_rejects_changed_bindings_semantics_authority_and_cancellation",
                        "qualification_preflight_requires_every_current_credential_and_lifecycle_control",
                        "qualification_preflight_preserves_uncertainty_and_never_resets_or_guesses_credits",
                    ],
                    ignored: false,
                });
            }
            "native-publication-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "native_preparation_recovers_original_acceptance_after_uncertain_put",
                        "native_publication_requires_authorized_predecessor_and_current_durable_denial",
                    ],
                    ignored: false,
                });
            }
            "native-daily-acceptance" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "native_acceptance_preserves_labels_receipts_values_and_complete_provenance",
                        "native_acceptance_rejects_gaps_closed_days_and_changed_capture_bindings",
                        "native_acceptance_enforces_reference_freshness_actions_and_exact_outlier_thresholds",
                        "native_acceptance_rechecks_authority_cancellation_and_original_admission",
                        "native_acceptance_requires_explicit_revision_chains_and_same_scoped_predecessor",
                        "native_artifact_decode_rejects_noncanonical_tampered_and_unaccepted_formats",
                    ],
                    ignored: false,
                });
            }
            "date-only-evidence" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "native_dates_preserve_reviewed_dst_buckets_and_exact_local_receipts",
                        "date_only_construction_rejects_unreviewed_or_conflicting_calendar_and_chronology",
                        "date_only_wire_decode_is_bounded_canonical_versioned_and_payload_safe",
                    ],
                    ignored: false,
                });
            }
            "twelve-data-conformance" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "tests::twelve_data::twelve_data_selected_request_is_raw_header_only_and_exact_native_capture_round_trips",
                        "tests::twelve_data::twelve_data_foreign_null_duplicate_incomplete_and_imprecise_responses_fail_closed",
                        "tests::twelve_data::twelve_data_errors_cancellation_and_authority_denial_never_retry_or_bypass_quota",
                        "tests::twelve_data::twelve_data_lost_confirmation_releases_original_receipt_without_refetch",
                        "tests::twelve_data::twelve_data_configuration_and_native_format_cannot_weaken_authority",
                    ],
                    ignored: false,
                });
            }
            "alpaca-conformance" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "tests::alpaca::alpaca_raw_sip_binding_preserves_exact_provider_instants_and_receipts",
                        "tests::alpaca::alpaca_pages_charge_separately_and_confirmation_loss_never_refetches",
                        "tests::alpaca::alpaca_manual_rate_rejection_never_sleeps_or_consumes_another_call",
                        "tests::alpaca::alpaca_malformed_foreign_duplicate_and_truncated_pages_fail_closed",
                        "tests::alpaca::alpaca_credentials_authority_historical_delay_and_spent_credits_deny_before_io",
                        "tests::alpaca::alpaca_errors_cancellation_and_uncertain_start_never_trigger_feed_fallback",
                    ],
                    ignored: false,
                });
            }
            "evidence-freezing" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "freezing_preserves_exact_quality_lineage_and_closed_calendar_coverage",
                        "corrections_link_immutable_predecessors_without_cross_scope_or_identity_reuse",
                    ],
                    ignored: false,
                });
            }
            "market-data-conformance" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "rights_permissions_scope_and_conflicts_fail_closed",
                        "latest_authority_revisions_never_resurrect_withdrawn_or_future_records",
                        "provider_capabilities_and_historical_knowledge_require_review",
                        "bounded_requests_cancellation_deadlines_and_retention_are_explicit",
                        "provider_results_cannot_expand_authority_or_bypass_provenance",
                        "synthetic_provider_port_requires_admission_and_post_call_authority",
                    ],
                    ignored: false,
                });
            }
            "market-data-quality" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "complete_explicit_calendars_preserve_dst_early_closes_and_raw_lineage",
                        "missing_closed_conflicting_and_unbounded_series_fail_without_partial_acceptance",
                        "bucket_end_grace_and_reference_freshness_have_exact_inclusive_boundaries",
                        "revisions_duplicates_and_cancellations_select_before_effective_filtering",
                        "action_coverage_and_point_in_time_reference_corrections_fail_closed",
                        "exact_outlier_and_independent_source_thresholds_do_not_adjust_or_choose_prices",
                    ],
                    ignored: false,
                });
            }
            "frozen-evaluation-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "actual_execution_and_settlement_expose_cost_latency_and_liquidity_sensitivity",
                        "repeated_and_checkpoint_resumed_paths_have_identical_canonical_reports",
                        "frozen_case_census_rejects_omitted_duplicate_and_multifactor_experiments",
                        "overlapping_holdouts_future_marks_and_invalid_continuations_fail_closed",
                        "partial_foreign_conflicting_and_open_owner_histories_produce_no_report",
                        "diagnostics_redact_financial_payloads_and_reports_preserve_declared_order",
                    ],
                    ignored: false,
                });
            }
            "option-analytics-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "independent_reference_premiums_and_greeks_cover_carry_and_intraday_expiry",
                        "analytic_greeks_satisfy_parity_and_independent_finite_differences",
                        "solver_requires_price_and_width_convergence_and_never_returns_an_exhausted_iterate",
                        "near_expiry_and_flat_time_value_cannot_fabricate_identified_volatility",
                        "nonfinite_degenerate_unsupported_and_unknown_inputs_fail_closed",
                        "retained_identities_reproduce_results_and_diagnostics_redact_inputs",
                    ],
                    ignored: false,
                });
            }
            "simulation-schema-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "admission_wire_schema_and_constructor_reject_unsupported_modes_and_invalid_facts",
                        "published_admission_schemas_retain_immutable_content_and_identity",
                        "published_execution_schemas_retain_immutable_content_and_identity",
                        "execution_wire_schema_and_constructor_reject_mutable_financial_input",
                        "settlement_wire_contracts_validate_balanced_and_bounded_committed_facts",
                        "published_settlement_schemas_retain_immutable_content_and_identity",
                    ],
                    ignored: false,
                });
            }
            "gateway-http" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "acknowledgement_waits_for_durable_admission",
                        "malformed_foreign_browser_and_oversized_inputs_never_reach_storage",
                        "conflicts_and_unavailable_commits_return_safe_non_success_responses",
                        "serving_requires_explicit_loopback_database_schema_and_scope",
                        "descriptor_commands_remain_available_without_serving_configuration",
                    ],
                    ignored: false,
                });
            }
            "gateway-query-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "query_scope_host_origin_and_invalid_ids_are_refused_before_upstream",
                        "queries_preserve_observations_and_exact_terminal_bytes_without_forwarded_identity",
                        "invalid_oversized_redirected_and_unavailable_projector_responses_fail_closed",
                        "stalled_projector_is_bounded_and_configuration_cannot_redirect_authority",
                    ],
                    ignored: false,
                });
            }
            "projector-query-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "internal_queries_reject_foreign_browser_hosts_and_writes_before_storage",
                        "configuration_descriptors_and_error_diagnostics_preserve_local_boundary",
                    ],
                    ignored: false,
                });
            }
            "research-lifecycle" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "explicit_lifecycle_time_order_and_final_states_are_irreversible",
                    ],
                    ignored: false,
                });
            }
            "research-projection-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "pending_terminal_replay_and_checkpoint_resume_preserve_exact_owner_bytes",
                        "conflicting_terminal_intent_foreign_scope_and_invalid_records_never_mutate",
                        "reordered_lifecycle_records_remain_explicit_gaps_without_cursor_or_state_advance",
                        "restore_rejects_corruption_wrong_anchor_versions_and_rechecks_sealed_outcomes",
                        "lifecycle_replay_duplicates_gaps_and_checkpoint_versions_preserve_original_bytes",
                    ],
                    ignored: false,
                });
            }
            "research-worker-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "verified_bytes_produce_deterministic_outcomes_and_wrong_digest_never_ranks",
                        "runtime_rejects_nonlocal_profiles_and_preserves_descriptor_commands",
                    ],
                    ignored: false,
                });
            }
            "research-rendering" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "canonical_artifact_and_abstention_match_frozen_bytes",
                        "permutations_ties_and_session_order_preserve_identical_output",
                        "mismatched_request_and_policy_bindings_fail_before_rendering",
                        "failed_required_evidence_abstains_without_partial_rankings",
                        "expiry_and_numeric_failure_never_publish_an_artifact",
                        "canonical_identity_retains_revision_and_availability_lineage",
                    ],
                    ignored: false,
                });
            }
            "research-momentum" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "exact_reference_vectors_include_positive_zero_and_negative_returns",
                        "half_ppm_ties_round_to_even_symmetrically",
                        "equivalent_scales_and_large_valid_intermediates_are_exact",
                        "score_range_is_checked_without_clamping_or_payload_diagnostics",
                        "complete_session_lookback_and_revision_are_retained",
                        "exact_rational_error_is_bounded_by_half_a_ppm",
                    ],
                    ignored: false,
                });
            }
            "research-evidence" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "complete_evidence_is_sealed_in_historical_order",
                        "time_rights_and_history_boundaries_fail_closed",
                        "revisions_units_lineage_and_duplicate_sessions_cannot_pass",
                        "reason_priority_is_stable_under_every_small_permutation",
                        "malformed_selection_and_oversized_evidence_are_bounded",
                    ],
                    ignored: false,
                });
            }
            "research-acquisition-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "request_and_outcome_reject_wrong_owner_route_scope_and_correlation",
                        "correlated_evidence_schema_matches_frozen_inventory",
                    ],
                    ignored: false,
                });
            }
            "research-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "frozen_records_round_trip_and_bind_exact_content",
                        "invalid_shapes_versions_bounds_and_semantics_are_rejected",
                        "raw_boundaries_reject_duplicates_noncanonical_and_oversize_without_leaking",
                        "published_schema_matches_frozen_v1_inventory",
                        "lifecycle_reasons_and_candidate_order_are_stable",
                    ],
                    ignored: false,
                });
            }
            "message-schema-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "generated_schemas_match_the_complete_committed_inventory",
                        "schema_inventory_rejects_stale_missing_extra_and_nonregular_artifacts",
                        "retained_baselines_preserve_schema_identity_and_existing_field_constraints",
                        "optional_additions_preserve_old_readers_and_default_new_reader_fields",
                        "compatibility_policy_rejects_breaking_and_unclassified_schema_mutations",
                        "typed_command_and_fact_round_trip_exact_golden_envelopes",
                        "payload_fixture_matrix_rejects_invalid_shapes_and_boundary_values",
                        "version_schema_producer_and_partition_mismatches_never_yield_typed_messages",
                        "constructor_serde_and_raw_byte_boundaries_cannot_bypass_validation",
                        "public_diagnostics_redact_payloads_and_rejected_metadata",
                    ],
                    ignored: false,
                });
            }
            "ignored-inventory" => {
                entry.discovery = Some(Discovery {
                    args: entry.command.args.clone(),
                    expected: proofs::ignored_inventory(),
                    ignored: true,
                })
            }
            "messaging-error-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "publish_error_constructors_preserve_all_categories_and_actual_source_presence",
                        "consume_error_constructors_preserve_all_categories_and_actual_source_presence",
                        "inbox_error_constructors_preserve_all_categories_and_actual_source_presence",
                        "outbox_error_constructors_preserve_all_categories_and_actual_source_presence",
                        "source_free_store_failure_terminates_the_inbound_processing_source_chain",
                        "public_messaging_errors_redact_causes_but_preserve_source_chains",
                    ],
                    ignored: false,
                });
            }
            "relay-observation-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec!["relay_observations_respect_validation_and_confirmed_outcomes"],
                    ignored: false,
                });
            }
            "image-smoke" | "sbom-images" => {
                entry.prerequisites = &[Requirement::Executable("docker")]
            }
            "relay-policy-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "attempt_budgets_validate_inclusive_limits_and_round_trip",
                        "backoff_validation_preserves_precision_and_rejects_invalid_intervals",
                        "owner_errors_are_startup_only_and_payload_safe",
                        "storage_failures_preserve_all_runtime_categories_and_real_source_presence",
                        "configuration_and_runtime_types_preserve_worker_auto_traits",
                    ],
                    ignored: false,
                });
            }
            "dependency-policy" => entry.prerequisites = &[Requirement::Executable("cargo-deny")],
            "sbom-rust" => entry.prerequisites = &[Requirement::Executable("cargo-cyclonedx")],
            "sqlx-metadata" => {
                entry.prerequisites = &[
                    Requirement::Environment("DATABASE_URL"),
                    Requirement::EnvironmentEquals("SQLX_OFFLINE", "false"),
                    Requirement::EnvironmentEquals("EDGEAGENT_SQLX_METADATA_LOCAL", "1"),
                    Requirement::Executable("docker"),
                    Requirement::Executable("cargo-sqlx"),
                ]
            }
            "secret-scan" | "secret-scanner-contracts" => {
                entry.prerequisites = &[Requirement::CommandProbe("gitleaks", &["version"])]
            }
            _ => (),
        }
    }
    checks.extend(proofs::checks());
    checks.extend(m08_checks());
    checks
}

// Expose actual registered audit targets to the coverage sensor, not a second
// allowlist that could stay green after a new workspace was added elsewhere.
pub(crate) fn dependency_policy_targets() -> Vec<(&'static str, &'static str)> {
    fn argument(args: &[&'static str], flag: &str, default: &'static str) -> &'static str {
        args.iter()
            .position(|argument| *argument == flag)
            .and_then(|index| args.get(index + 1))
            .copied()
            .unwrap_or(default)
    }

    checks()
        .into_iter()
        .filter(|check| {
            check.profiles.contains(&"dependency-audit")
                && check.command.program == Program::Cargo
                && check.command.args.first() == Some(&"deny")
        })
        .map(|check| {
            (
                argument(&check.command.args, "--manifest-path", "Cargo.toml"),
                argument(&check.command.args, "--config", "deny.toml"),
            )
        })
        .collect()
}

fn m08_checks() -> Vec<Check> {
    let mut checks = Vec::new();
    for (profile, manifest, fmt_id, lint_id, test_id, audit_id) in [
        (
            "m08-event-spine-proof",
            "experiments/event-spine-proof/Cargo.toml",
            "m08-event-spine-format",
            "m08-event-spine-clippy",
            "m08-event-spine-test",
            "m08-event-spine-dependency-policy",
        ),
        (
            "m08-durable-execution",
            "experiments/durable-execution/Cargo.toml",
            "m08-duroxide-format",
            "m08-duroxide-clippy",
            "m08-duroxide-test",
            "m08-duroxide-dependency-policy",
        ),
        (
            "m08-dbos-proof",
            "experiments/dbos-proof/Cargo.toml",
            "m08-dbos-format",
            "m08-dbos-clippy",
            "m08-dbos-test",
            "m08-dbos-dependency-policy",
        ),
        (
            "m08-temporal-proof",
            "experiments/temporal-proof/Cargo.toml",
            "m08-temporal-format",
            "m08-temporal-clippy",
            "m08-temporal-test",
            "m08-temporal-dependency-policy",
        ),
        (
            "m08-flawless-proof",
            "experiments/flawless-proof/Cargo.toml",
            "m08-flawless-format",
            "m08-flawless-clippy",
            "m08-flawless-test",
            "m08-flawless-dependency-policy",
        ),
    ] {
        let mut audit = check(
            audit_id,
            "T3",
            &["dependency-audit"],
            Program::Cargo,
            &[
                "deny",
                "--manifest-path",
                manifest,
                "--locked",
                "--workspace",
                "--config",
                "experiments/deny.toml",
                "check",
            ],
            "Current isolated proof advisory/license/source/bans policy; requires network and cargo-deny, not a runtime proof.",
        );
        audit.prerequisites = &[Requirement::Executable("cargo-deny")];
        checks.push(audit);
        // Profiles are static policy values; each generated check retains one static slice below.
        let static_profiles: &'static [&'static str] = match profile {
            "m08-event-spine-proof" => &["m08-event-spine-proof", "m08-event-spine-proof-static"],
            "m08-durable-execution" => &["m08-durable-execution", "m08-durable-execution-static"],
            "m08-dbos-proof" => &["m08-dbos-proof", "m08-dbos-proof-static"],
            "m08-temporal-proof" => &["m08-temporal-proof", "m08-temporal-proof-static"],
            _ => &["m08-flawless-proof", "m08-flawless-proof-static"],
        };
        let test_profiles: &'static [&'static str] = match profile {
            "m08-event-spine-proof" => &["m08-event-spine-proof", "m08-event-spine-proof-test"],
            "m08-durable-execution" => &["m08-durable-execution", "m08-durable-execution-test"],
            "m08-dbos-proof" => &["m08-dbos-proof", "m08-dbos-proof-test"],
            "m08-temporal-proof" => &["m08-temporal-proof", "m08-temporal-proof-test"],
            _ => &["m08-flawless-proof", "m08-flawless-proof-test"],
        };
        checks.push(check(
            fmt_id,
            "T3",
            static_profiles,
            Program::Cargo,
            &["fmt", "--manifest-path", manifest, "--all", "--check"],
            "Isolated M08 proof formatting, not root workspace acceptance.",
        ));
        checks.push(check(
            lint_id,
            "T3",
            static_profiles,
            Program::Cargo,
            &[
                "clippy",
                "--locked",
                "--workspace",
                "--manifest-path",
                manifest,
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            "Isolated proof lock and lint policy, not crash recovery.",
        ));
        let mut proof = check(
            test_id,
            "T3",
            test_profiles,
            Program::Cargo,
            &[
                "test",
                "--locked",
                "--workspace",
                "--manifest-path",
                manifest,
                "--all-targets",
            ],
            "Self-provisioned synthetic M08 proof; cold cache needs network; Temporal/Flawless fetch digest-checked binaries, Flawless provisions Wasm; no model calls.",
        );
        if matches!(profile, "m08-event-spine-proof" | "m08-dbos-proof") {
            proof.prerequisites = &[Requirement::Executable("docker")];
        }
        let expected = match profile {
            "m08-event-spine-proof" => {
                vec!["m08_event_spine_alone_does_not_resume_post_ack_step_b"]
            }
            "m08-durable-execution" => vec![
                "completed_step_is_not_reexecuted_after_worker_process_crash",
                "cancellation_of_in_flight_b_is_terminal_without_completing_b",
            ],
            "m08-dbos-proof" => vec![
                "completed_step_is_not_reexecuted_after_worker_process_exit",
                "application_row_and_dbos_enqueue_commit_or_rollback_together",
            ],
            "m08-temporal-proof" => {
                vec![
                    "worker_crash_during_model_step_resumes_without_reexecuting_preparation",
                    "server::tests::shutdown_reaps_unwaited_and_completed_children",
                ]
            }
            _ => vec![
                "tests::recorded_effects_resume_after_server_process_crash",
                "tests::download::tests::exhausted_body_retries_remove_all_partial_bytes",
                "tests::download::tests::terminal_rejections_never_publish_an_executable_or_retry",
                "tests::download::tests::transient_status_retries_with_the_same_pinned_digest",
                "tests::download::tests::truncated_body_restarts_fresh_and_exposes_only_verified_binary",
            ],
        };
        proof.command.args.push("--");
        let mut args = proof.command.args.clone();
        args.push("--list");
        proof.discovery = Some(Discovery {
            args,
            expected,
            ignored: false,
        });
        checks.push(proof);
    }
    checks
}

pub(super) fn validate(checks: &[Check]) -> Result<(), String> {
    let mut ids = std::collections::BTreeSet::new();
    for check in checks {
        if check.id.is_empty()
            || !ids.insert(check.id)
            || check.command.args.is_empty()
            || check.evidence.is_empty()
        {
            return Err(format!("invalid or duplicate check: {}", check.id));
        }
        if check.profiles.is_empty() || check.profiles.iter().any(|p| !PROFILES.contains(p)) {
            return Err(format!("unknown or absent profile for {}", check.id));
        }
        if check.profiles.contains(&"verify")
            && (check.tier == "T3" || !check.prerequisites.is_empty())
        {
            return Err(format!(
                "default acceptance must be service/credential-free: {}",
                check.id
            ));
        }
        if !matches!(check.tier, "T1" | "T2" | "T3") {
            return Err(format!("unknown tier for {}", check.id));
        }
        if let Some(discovery) = &check.discovery {
            if discovery.expected.is_empty()
                || discovery
                    .expected
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != discovery.expected.len()
                || discovery
                    .args
                    .iter()
                    .filter(|argument| **argument == "--list")
                    .count()
                    != 1
                || discovery.args.last() != Some(&"--list")
                || !discovery.args.contains(&"--")
            {
                return Err(format!("invalid discovery contract for {}", check.id));
            }
            if discovery.ignored != discovery.args.contains(&"--ignored")
                || discovery.args.contains(&"--exact")
            {
                return Err(format!("invalid discovery scope for {}", check.id));
            }
            if check.id == "ignored-inventory" {
                if check.command.program != Program::Cargo || check.command.args != discovery.args {
                    return Err(
                        "ignored census must execute its exact discovery contract".to_owned()
                    );
                }
            } else {
                if check.command.args.contains(&"--list") {
                    return Err(format!("list-only execution cannot prove {}", check.id));
                }
                let mut expected_command = discovery.args.clone();
                expected_command.retain(|argument| *argument != "--list");
                if let Some(exact) = check.command.args.iter().position(|a| *a == "--exact") {
                    if exact + 2 != check.command.args.len()
                        || !discovery.expected.contains(&check.command.args[exact + 1])
                    {
                        return Err(format!(
                            "exact filter lacks protected identity for {}",
                            check.id
                        ));
                    }
                    expected_command.extend(["--exact", check.command.args[exact + 1]]);
                }
                if expected_command != check.command.args || check.command.program != Program::Cargo
                {
                    return Err(format!(
                        "discovery and execution argv differ for {}",
                        check.id
                    ));
                }
            }
        }
    }
    for profile in PROFILES {
        if !checks.iter().any(|c| c.profiles.contains(profile)) {
            return Err(format!("empty profile: {profile}"));
        }
    }
    Ok(())
}
