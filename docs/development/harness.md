# EdgeAgent coding harness

## TL;DR

- Use the [documentation map](../README.md) to load only the guidance relevant
  to the task.
- Run `make repository` and `make architecture` for immediate structural
  feedback while editing.
- Run `make verify` before handoff; run image, local-platform, and supply-chain
  checks when their boundaries change.
- Report every unavailable check as not run with its blocker and residual risk.
- Turn recurring review findings into clearer guidance, an early deterministic
  sensor, a behavioral test, or an explicit owned exception.

## Purpose

The coding harness makes the repository understandable and self-checking for
human contributors and coding agents. Feed-forward guidance explains intent
and constraints before editing. Feedback sensors detect structural violations
and behavioral regressions after a change. The shortest useful loop runs cheap
checks first while preserving one documented acceptance command.

This guide inventories the harness. It does not replace the canonical workflow
in [CONTRIBUTING.md](../../CONTRIBUTING.md), implementation practice in the
[engineering standards](engineering-standards.md), or the exact automation in
the [repository-quality workflow](../../.github/workflows/repository-quality.yml).

## Cost and timing tiers

| Tier | Intended timing | EdgeAgent feedback |
|:--|:--|:--|
| T0 — route | Before editing | Read-only task routing through the [documentation map](../README.md), followed by the relevant architecture pages and ADRs. |
| T1 — structure | During editing | `make repository` and `make architecture`; deterministic checks that require no database, broker, cloud account, or network once dependencies are present. |
| T2 — acceptance | Before handoff or pull request | `make verify`; formatting, repository and architecture policy, static contract checks, locked builds, Clippy, Python tests, and Rust tests. Cargo may fetch missing locked dependencies. |
| T3 — specialized | When an affected boundary requires it | `make image-smoke`, local Compose conformance, `make supply-chain`, release validation, and explicitly scoped security, performance, or cloud-profile checks. |

A tier describes cost and timing, not permission to skip an applicable check.
Follow the constrained-environment reporting rules in
[CONTRIBUTING.md](../../CONTRIBUTING.md#verification-and-constrained-environments)
when a required check cannot run.

## Feed-forward guidance

| Guide | What it supplies | Load when |
|:--|:--|:--|
| [AGENTS.md](../../AGENTS.md) | Concise tool-facing agreement and canonical links | Every task |
| [CONTRIBUTING.md](../../CONTRIBUTING.md) | Workflow, issue shape, verification, review, and completion policy | Every task |
| [Documentation map](../README.md) | Progressive task-to-source routing | Every task |
| [Product vision](../product/vision.md) | Users, outcomes, boundaries, and non-goals | Product behavior or terminology changes |
| [System architecture](../architecture/system-overview.md) | Components, control flow, trust, and ownership | Code or boundary changes |
| [Dependency rules](../architecture/dependency-rules.md) | Allowed workspace package roles and edges | Crate or direct dependency changes |
| [Engineering standards](engineering-standards.md) | Rust design, tests, documentation, security, and operations | Implementation work |
| [Local platform](local-platform.md) | Dependency lifecycle and conformance environment | Integration or recovery work |
| [ADR index](../decisions/README.md) | Accepted durable decisions and supersession history | Decisions relevant to the task |
| [Milestones](../roadmap/milestones.md) | Intended outcome sequence | Planning and issue drafting |
| [Security policy](../../SECURITY.md) | Private vulnerability reporting and security expectations | Security-sensitive work |

Guidance states intent, boundaries, and where its claims are verified. Keep each
normative rule in one canonical document and link to it from concise entry
points.

## Feedback sensors

Contributors may run a focused test while editing, but run every applicable
acceptance check before handoff.

| Command or sensor | What it proves | Tier and dependencies |
|:--|:--|:--|
| `python scripts/verify_repository.py` | Text encoding, line endings, final newlines, required repository paths, and a simple independent documentation baseline remain valid | T1; Python 3.11+ |
| `cargo xtask repository` | Rendered local Markdown links and anchors, TL;DR placement, ADR and milestone lifecycle/index policy, and stable GitHub authoring templates satisfy mechanical policy | T1; Rust toolchain and locked dependencies |
| `python scripts/verify_architecture.py` | Workspace manifests use the exact reviewed package, metadata, target, and direct dependency declarations | T1; Python 3.11+ |
| `cargo xtask architecture` | Cargo's normalized internal graph satisfies package roles, dependency classes, temporary-exception policy, and cycle rules | T1; Rust toolchain and Cargo metadata |
| `make verify` | The complete default repository acceptance gate passes with the locked graph | T2; Python, Rust toolchain, `make`, and locally cached or downloadable locked crates |
| `make image-smoke` | Every deployable image builds and runs under the declared non-root, read-only, networkless contract | T3; Docker |
| `make local-up` and focused conformance tests | PostgreSQL, NATS, object storage, telemetry, and restart behavior satisfy local integration contracts | T3; Docker with Compose |
| `cargo sqlx prepare --workspace` plus `.sqlx/` diff check | Compile-checked inbox and outbox queries match the combined checked-in PostgreSQL migrations and leave no stale metadata | T3; pinned SQLx CLI and isolated PostgreSQL; see the [SQLx metadata guide](sqlx-metadata.md) |
| `make supply-chain` | Dependency/advisory/license policy and the complete Rust and image SBOM set pass | T3; pinned tools, network for current advisories, and Docker |
| `cargo test --locked --manifest-path experiments/<candidate>/Cargo.toml` | Isolated M08 durable-execution decision proof for one candidate, including real worker-process restart or DBOS atomic enqueue | T3; see the [experiment guide](../../experiments/README.md) for container and cold-cache network requirements |

The command order and unavailable-check protocol remain canonical in
[CONTRIBUTING.md](../../CONTRIBUTING.md#verification-and-constrained-environments).
CI runs repository policy before architecture and all heavier jobs.

The Rust workspace tests include T2 `Debug` redaction contracts for payload-
bearing envelope, delivery, publication, outbox, quarantine, and replay values.
Synthetic sentinels must not appear as text or formatted numeric byte arrays;
safe byte counts remain available. Add a focused test when a new payload
carrier is introduced; a source-text ban on `derive(Debug)` would be too noisy
to prove this behavior.
The contracts T2 tests also inject synthetic private text through failing
Serde implementations, malformed JSON, and mismatched routing metadata. Public
error `Display` and `Debug` must retain the failure category without echoing
that text. The messaging error-redaction integration test and downstream
wrapper tests inject causes whose `Display` and `Debug` contain sentinels, then
check that public error formatting hides them while `Error::source()` still
preserves the causes. When adding a public error wrapper, test both paths: a
redacted top-level message does not make its raw source chain safe to log.

The T2 messaging contract tests exercise both inclusive retry-delay bounds and
adjacent invalid values. A disposition must contain a validated delay, so an
invalid caller input cannot consume a delivery before broker settlement.
They also assert that broker-metadata or oversized-payload protocol faults halt
the consumer's pre-pull gate; a caller retry loop cannot silently consume
another delivery.
The `FailureCode` boundary tests cover the inclusive 1–64-byte token range and
reject uppercase, whitespace, non-ASCII, and oversized codes. Its compile-fail
doctest ensures an invalid service-owned constant cannot reach inbound or
outbound persistence. CI runs workspace doctests explicitly because the
`--all-targets` unit-test gate does not establish that contract.
The portable metadata tests exercise distinct identity, route, and attempt
mapping, the inclusive 512-byte text bound, non-ASCII and non-graphic rejection,
and positive attempt parsing. NATS adapter tests separately reject invalid
stream and consumer sequences; the infallible portable assembly signature
prevents primitive field transposition without requiring broker-only counters.
The delivery-ownership integration test runs in its own executable because
tracing callsite interests are process-global and parallel unit tests can race
its subscriber registration. It records warnings on unsettled drop, requires
no warning after settlement ownership transfers, and rejects both textual and
numeric-byte payload leakage from the warning fields.

The contracts crate has an ignored, opt-in throughput probe for comparing
clone-based and borrowed typed decoding:

```text
cargo test --release --locked -p edgeagent-contracts --lib payload_decode_throughput_probe -- --ignored --nocapture
```

Its timings are machine-dependent evidence, not a CI threshold. The default
suite asserts byte-for-byte canonical fixture encoding. The T2 `make verify`
gate and the Linux/Windows repository-quality job also run
`cargo test --locked -p edgeagent-contracts --features serde_json/preserve_order`.
This catches dependency feature unification that could otherwise change
persisted envelope bytes without changing contract source code.

The local-platform CI job asserts the named test exists and runs the opt-in
PostgreSQL outbox lease-fencing regression as a T3 behavioral contract. It
forces an existing lease to expire in the test database, reclaims the message
under the same worker token, and requires stale publish, retry, and quarantine
attempts to fail without
changing the new claim. It also verifies that operator replay resets the
attempt budget without resetting the fencing generation. Run it against only
the isolated local PostgreSQL profile:

```text
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-outbox-postgres --test postgres_outbox -- --ignored --exact same_owner_reclaim_rejects_stale_transitions_and_preserves_replay_fence
```

## Repository policy

The two repository sensors have complementary ownership. The Python verifier
checks bytes and required inventory. `cargo xtask repository` parses CommonMark
and reports semantic structure violations in stable path and line order. Do not
weaken one sensor because the other covers adjacent behavior.

The Rust sensor checks these rules without network access:

- repository-local Markdown destinations are source-relative, avoid directories
  excluded from policy discovery, use exact path capitalization, remain inside
  the repository, and name an existing GitHub-style heading anchor when a
  fragment is present; fragments on non-Markdown targets are rejected because
  the sensor cannot prove a rendered heading;
- the Markdown inventory rejects non-excluded symbolic links to Markdown files
  and directories so policy scope remains consistent across platforms;
- a long or policy-sensitive document begins its visible level-two sections
  with `## TL;DR` under the criteria in
  [CONTRIBUTING.md](../../CONTRIBUTING.md#documentation);
- ADRs use the published identity ledger, metadata, required-section,
  contiguous-number, single-table index, and reciprocal supersession contracts
  in the [ADR policy](../decisions/README.md);
- milestone entries use one rendered index table, contiguous `MNN - Outcome`
  identifiers, the published high-water mark, and matching direct boundary
  headings and outcome statements; and
- issue forms and the pull request template retain their stable fields,
  commands, prompts, and review checklist.

Rendered structure matters. Comments, images, and fenced code cannot satisfy a
required visible heading or summary, and raw HTML cannot appear in a required
summary body. The check intentionally ignores external URL availability and
links expressed only through raw HTML. A passing repository check proves
document structure and local navigation, not factual accuracy or freshness.

## Steering loop

Classify a recurring review finding or escaped defect and close the cheapest
reliable loop:

1. If intent is unclear, improve the narrow canonical guide and its route.
2. If a structural rule is deterministic, add or improve an actionable T1
   sensor.
3. If observable behavior regressed, add a focused test or fixture at the
   lowest convincing boundary.
4. If automation would be noisy or misleading, record the exception, owner,
   reason, and review trigger.

A sensor failure identifies the rule, affected location, accepted shape,
likely correction, and canonical guidance. Sensors must not rewrite expected
behavior, weaken an invariant, depend on ambient global tools, or report an
unavailable check as passed.

Review harness changes like product changes. Watch false positives, repeated
suppressions, runtime, flaky retries, stale routes, and findings that continue
to escape. Promote a sensor to a blocking gate only when it is deterministic,
actionable, and owned.

## Multi-PR execution plans

Use a versioned execution plan when one approved outcome needs several
dependent pull requests, especially for migrations, compatibility transitions,
or architectural refactoring. Add the plan under `docs/development/plans/`
with the first implementation slice; do not create an empty plans directory.

The plan records the outcome, invariants, exclusions, relevant ADRs, ordered
slices, compatibility, rollout and rollback, verification, and discoveries
that change the sequence. Each slice remains independently reviewable and
leaves the repository buildable. GitHub issues and milestones remain
authoritative for delivery state, and ADRs remain authoritative for durable
decisions.

## Extending the harness

Add guidance or a sensor only when it closes a demonstrated gap. Prefer
extending an existing canonical document or `cargo xtask` command over creating
a parallel tool. A new sensor is deterministic and cross-platform where its
tier is cross-platform, uses repository-pinned dependencies, tests useful
failure diagnostics, and states which tier and CI job own it.

If a new language workspace or deployable component is introduced, connect
its checks to the appropriate documented tier. Contributors must not discover
a hidden acceptance command only after CI fails. Avoid an LLM-as-judge gate,
global coverage or file-size thresholds, and larger mandatory context bundles;
use deterministic sensors and focused behavior tests instead.
