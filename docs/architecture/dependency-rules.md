# Workspace dependency rules

## TL;DR

- Run `cargo xtask architecture` after adding a workspace package or changing a
  Cargo dependency.
- Every workspace package has one architectural role, and every direct
  dependency between workspace packages is explicit policy.
- Normal, development, and build dependencies are evaluated independently;
  development-only conformance composition cannot authorize a production edge.
- `scripts/verify_architecture.py` checks every direct dependency declaration,
  including third-party build, optional, and target-specific dependencies;
  the Rust check validates Cargo's normalized internal graph.
- The current policy contains no temporary exceptions.

## Package roles

| Package | Role | Responsibility |
|---|---|---|
| `edgeagent-contracts` | Contract | Stable component identities, descriptors, and validated CloudEvents envelopes |
| `edgeagent-market-data-domain` | Domain | Checked market values and observation/provenance primitives; no transport, persistence or provider types |
| `edgeagent-simulation-domain` | Domain | Frozen runs, virtual time, reproducible continuation and simulated-account risk policy; no effects or storage |
| `edgeagent-event-spine-conformance` | Tooling | Synthetic contracts, schema tooling and finite process-proof composition; not a deployable |
| `edgeagent-messaging` | Port | Application-facing messaging, relay-storage, and atomic inbound-store ports plus settlement semantics |
| `edgeagent-telemetry` | Observability | Exporter-neutral event-spine measurements and correlated diagnostic facts |
| `edgeagent-inbox-handler` | Application | Persistence-neutral policy for validation, atomic processing, retry, quarantine, and settlement |
| `edgeagent-outbox-relay` | Application | Persistence-neutral publication, lease, retry, and terminal-quarantine policy |
| `edgeagent-inbox-postgres` | Adapter | PostgreSQL transactions and durable inbox, quarantine, and replay evidence |
| `edgeagent-outbox-postgres` | Adapter | PostgreSQL outbox persistence, leasing, outcomes, quarantine, and replay evidence |
| `edgeagent-messaging-nats` | Adapter | NATS JetStream publication, pull delivery, and confirmed settlement |
| `edgeagent-service-runtime` | Runtime support | Common process bootstrap and component-description behavior |
| `edgeagent-gateway` | Composition root | Gateway process configuration and concrete wiring |
| `edgeagent-market-data` | Composition root | Market-data process configuration and concrete wiring |
| `edgeagent-research` | Composition root | Research process configuration and concrete wiring |
| `edgeagent-execution-simulator` | Composition root | Dry-run execution process configuration and concrete wiring |
| `edgeagent-audit-projector` | Composition root | Audit projection process configuration and concrete wiring |
| `edgeagent-xtask` | Tooling | Repository-owned checks that do not enter production binaries or images |

Roles describe dependency responsibility, not a requirement to create another
layer or interface. A package is added only with observable behavior and its
role and exact dependency policy in the same change.

## Enforced internal graph

The policy in `crates/xtask/src/architecture/policy.rs` permits these normal
dependencies between workspace packages:

| Package | Allowed normal workspace dependencies |
|---|---|
| `edgeagent-contracts` | None |
| `edgeagent-market-data-domain` | None |
| `edgeagent-simulation-domain` | `edgeagent-market-data-domain` |
| `edgeagent-event-spine-conformance` | `edgeagent-contracts`, `edgeagent-inbox-handler`, `edgeagent-inbox-postgres`, `edgeagent-messaging`, `edgeagent-messaging-nats`, `edgeagent-outbox-postgres`, `edgeagent-outbox-relay`, `edgeagent-telemetry` |
| `edgeagent-messaging` | `edgeagent-contracts` |
| `edgeagent-telemetry` | `edgeagent-contracts`, `edgeagent-messaging` |
| `edgeagent-inbox-handler` | `edgeagent-contracts`, `edgeagent-messaging`, `edgeagent-telemetry` |
| `edgeagent-outbox-relay` | `edgeagent-contracts`, `edgeagent-messaging`, `edgeagent-telemetry` |
| `edgeagent-inbox-postgres` | `edgeagent-contracts`, `edgeagent-messaging` |
| `edgeagent-outbox-postgres` | `edgeagent-contracts`, `edgeagent-messaging` |
| `edgeagent-messaging-nats` | `edgeagent-contracts`, `edgeagent-messaging` |
| `edgeagent-service-runtime` | `edgeagent-contracts` |
| Each service composition root | `edgeagent-contracts`, `edgeagent-service-runtime` |
| `edgeagent-xtask` | None |

Two development-only compositions support adapter and recovery conformance:

- `edgeagent-inbox-handler` may use `edgeagent-inbox-postgres`,
  `edgeagent-messaging-nats`, `edgeagent-outbox-postgres`, and
  `edgeagent-outbox-relay` as development dependencies; and
- `edgeagent-outbox-relay` may use `edgeagent-outbox-postgres` as a development
  dependency.

The conformance tooling binary uses the exact normal adapter/application edges
above to launch independent proof processes. No service imports conformance
tooling; these edges authorize no production domain dependency or OCI deployable.
[ADR-0026](../decisions/0026-compose-durable-acceptance-in-independent-processes.md)
records this composition boundary and restricted fixture runtime identities.

These edges assemble realistic tests; they do not invert production ownership.
There are no allowed internal build dependencies.

## Enforcement responsibilities

`cargo xtask architecture` loads the locked workspace with `cargo_metadata` and
compares Cargo's normalized dependency graph with the reviewed policy. It
rejects:

- a workspace package without a role and dependency policy;
- policy for a package that no longer exists;
- an internal dependency absent from the allowlist for its dependency kind;
- an allowlisted edge that is no longer declared; and
- a cycle in normal and build dependencies.

The check identifies workspace dependencies by package path, so a renamed Cargo
dependency cannot bypass policy. Optional and target-specific declarations are
included even when inactive on the current host.

The existing `python scripts/verify_architecture.py` check remains a separate,
complementary gate. It verifies the exact workspace member paths, package names,
target types, inherited metadata and lints, publication settings, and exact
canonical dependency identities, including third-party crates. It walks normal,
development, and build tables at the top level and under every target predicate
or target triple. Inactive targets and optional declarations are checked without
enabling features or compiling dependencies. Rust 2024's unsupported
`dev_dependencies` and `build_dependencies` spellings are rejected with their
hyphenated replacements.

Each package's `PackageRule` has separate normal, development, and build
allowances. Required identities are compared with the union of declarations
for that kind across all targets; each individual declaration must have a
kind-specific allowance. A normal or development allowance never authorizes a
build dependency. The current manifest policy permits no build dependencies.

Identity comes from a declaration's `package` field or dependency key. An
inherited declaration resolves its key through `[workspace.dependencies]`
first, including any package rename there. Reviewed identities are fixed in
checker policy, not inferred as permissions from the workspace manifest:
`cloudevents` resolves to the approved `cloudevents-sdk`, while disguising `sqlx`
under an approved key remains an error. Inherited declarations may override
only `optional`, `features`, and `default-features`; missing inheritance entries
and malformed identity or dependency-table shapes fail closed. Diagnostics
include the package manifest, kind/target table, alias, resolved identity, and
optional/inheritance state for an unapproved declaration.

The Rust check owns Cargo-normalized internal roles, edge classes, stale-policy
detection, and production-cycle detection. Real, temporary Cargo workspace
fixtures prove its treatment of renamed, inherited, optional, and inactive
target declarations without registry packages or services. Both architecture
commands run locally and in CI through the existing T1/T2 gates.

These sensors protect declared capability boundaries, not source provenance,
transitive capabilities, feature safety, module cohesion, or runtime access.
Canonical identity unions do not distinguish duplicate declarations or package
versions. Cargo validation, locked resolution, supply-chain policy, and review
remain required for those concerns.

## Changing dependencies

When a change adds, removes, or reclassifies a dependency:

1. identify the consuming capability and package role;
2. confirm that production dependencies point toward stable contracts or
   application policy, or remain confined to an adapter, composition root, or
   repository tool;
3. update the exact Rust policy and the Python manifest policy in the same pull
   request;
4. record the need, features, license, maintenance status, security posture,
   replacement boundary, and exit path for a new third-party crate; and
5. run `cargo xtask architecture` and the complete repository quality gate.

No temporary exception is configured. A future exception requires an accepted
ADR, an exact package pair and dependency class, an owner and removal condition,
and a diagnostic on every successful check. An exception never authorizes other
edges between packages that happen to share the same roles.

The tooling-only `edgeagent-xtask` dependency on `yaml-rust2` 0.13.0, with
default features disabled, supports the semantic GitHub workflow security
sensor. It provides maintained YAML 1.2 parsing instead of a line-oriented
parser; it enters no production crate or image. Parser input bounds, unsupported
YAML forms, payload-safe diagnostics, exact manifest policy, and mutation tests
form its replacement boundary. A parser or feature change requires reviewing
these contracts with [ADR-0021](../decisions/0021-enforce-ci-security-and-secret-scanning.md).

## Related decisions

The domain requests Chrono 0.4.45 and Chrono-TZ 0.10.4 (both MIT OR Apache-2.0),
with default features disabled and only `std` requested. Chrono supplies checked
Gregorian dates and UTC instants. P02 adds Chrono-TZ for deterministic New York
local expiry dates across DST/UTC-midnight boundaries, rather than hand-written
offset rules. Its [published package](https://crates.io/crates/chrono-tz/0.10.4)
and [upstream license](https://github.com/chronotope/chrono-tz/blob/main/LICENSE)
were reviewed for this use; the maintained Chronotope project uses generated,
bundled IANA rules, not OS timezone data or network lookup. Pin/expose that data
revision and reject mismatches; this is not a claim of latest exchange rules.
Do not enable optional timezone generation, serde or case-insensitive features.
Both libraries' types stay private; replacing the local-date conversion does
not expose their API. No clock or locale lookup is performed. Both sensors
reject unapproved normal, development and build capabilities, including aliases
and inactive target declarations. Feature unification and transitive capability
review remain separate obligations, not guarantees of these identity sensors.

Conformance roots alone pin OpenTelemetry API/SDK/OTLP 0.33.1, Reqwest 0.13.5
(blocking, no default TLS/proxy features), and Tracing-subscriber 0.3.23
(registry/std). Test-only Axum 0.8.9, OTLP models 0.33.1 and Prost 0.14.4 decode
the owned HTTP/protobuf receiver. These Apache-2.0 and/or MIT dependencies are
confined to tooling and use reviewed minimal features. The facade, application
and product runtime crates acquire no exporter SDK. The allowlisted projection,
bounded OTLP protocol and protected process proofs form the replacement boundary;
[ADR-0029](../decisions/0029-export-bounded-correlated-event-spine-observations.md)
records maintenance, lifecycle, privacy and rollout constraints.

The conformance-only package pins Schemars 1.2.3 (derive/std only) and Jsonschema
0.58.6 (default features disabled). They derive/validate local schemas, enter no
production dependency graph or image, and enable no HTTP/file schema resolver.
Serde wire types, payload-safe errors, schema artifacts/baselines, and the
protected `message-schema-contracts` gate form the replacement boundary.
[ADR-0024](../decisions/0024-define-synthetic-event-spine-acceptance-contracts.md)
records license/feature review and compatibility limits. Updating either tool
must regenerate and review current artifacts without rewriting frozen history.

- [ADR-0001: Use a Rust workspace with multiple deployables](../decisions/0001-use-a-rust-workspace-with-multiple-deployables.md)
- [ADR-0006: Keep inbound coordination persistence-neutral](../decisions/0006-keep-inbound-coordination-persistence-neutral.md)
- [ADR-0007: Enforce workspace capability dependencies](../decisions/0007-enforce-workspace-capability-dependencies.md)
