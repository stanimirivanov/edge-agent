# Workspace dependency rules

## TL;DR

- Run `cargo xtask architecture` after adding a workspace package or changing a
  Cargo dependency.
- Every workspace package has one architectural role, and every direct
  dependency between workspace packages is explicit policy.
- Normal, development, and build dependencies are evaluated independently;
  development-only conformance composition cannot authorize a production edge.
- `scripts/verify_architecture.py` continues to enforce manifest and repository
  structure while the Rust check validates Cargo's normalized internal graph.
- The current policy contains no temporary exceptions.

## Package roles

| Package | Role | Responsibility |
|---|---|---|
| `edgeagent-contracts` | Contract | Stable component identities, descriptors, and validated CloudEvents envelopes |
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
target types, inherited metadata and lints, publication settings, and complete
normal and development dependency keys, including third-party crates. The Rust
check owns Cargo-normalized internal roles, edge classes, stale-policy detection,
and production-cycle detection. Both commands run locally and in CI.

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

## Related decisions

- [ADR-0001: Use a Rust workspace with multiple deployables](../decisions/0001-use-a-rust-workspace-with-multiple-deployables.md)
- [ADR-0006: Keep inbound coordination persistence-neutral](../decisions/0006-keep-inbound-coordination-persistence-neutral.md)
- [ADR-0007: Enforce workspace capability dependencies](../decisions/0007-enforce-workspace-capability-dependencies.md)
