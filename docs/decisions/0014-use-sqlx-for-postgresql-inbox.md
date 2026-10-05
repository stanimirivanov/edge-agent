# ADR-0014: Use SQLx for the PostgreSQL inbox adapter

- Status: Accepted
- Date: 2026-10-05
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use pinned SQLx for inbox, quarantine, and replay queries and for the adapter-owned transaction.
- Keep the persistence-neutral `InboundMessageStore` contract and atomic first-delivery behavior established by [ADR-0006](0006-keep-inbound-coordination-persistence-neutral.md).
- Expose a mutable SQLx transaction only at the PostgreSQL-specific service callback; portable application code receives no driver type.
- Use bound parameters and typed row mappings now. PostgreSQL-backed CI tests validate queries against migrations; offline `query!` metadata is deferred.

## Context

The inbox adapter previously assembled positional `tokio-postgres` parameters and decoded rows by index. Schema drift could only be discovered at runtime, and the callback's driver transaction prevented adopting SQLx for one query without losing the single-transaction inbox-plus-service invariant. The crate has no production callers yet; its consumers are repository conformance tests. A coherent driver replacement is therefore possible without a persisted-data or wire migration.

SQLx provides explicit parameter binding, typed row mapping, and transaction ownership in Rust. Its [compile-time query macros](https://docs.rs/sqlx/0.9.0/sqlx/macro.query.html#requirements) require a database schema at build time or checked-in offline query metadata. The repository's default quality gate must remain independent of PostgreSQL, and this increment has no portable database available for generating and checking that metadata. Runtime SQLx queries plus PostgreSQL conformance tests are an honest intermediate guarantee, not a claim of compile-time SQL validation.

## Decision

`edgeagent-inbox-postgres` uses SQLx 0.9.0 with PostgreSQL, Tokio, rustls with native roots, and derive features. The adapter begins, commits, and rolls back `sqlx::Transaction<Postgres>`. Its methods bind every message or operator value and map selected rows into private typed structures. `PostgresTransactionalMessageHandler` receives a mutable reference to that same transaction for service-owned writes and transactional outbox enqueue. It must not create a second connection for writes that must commit with inbox identity.

The semantic `InboundMessageStore` port and broker settlement order remain unchanged. SQLSTATE classification preserves fail-closed invariants and retryable or ambiguous outcomes. SQLx encode, decode, and column-mapping errors are invariants; connection and protocol loss are unavailable. A failed rollback remains ambiguous. Error causes remain accessible through the Rust source chain, not public text or `Debug`.

The existing outbox adapter remains on `tokio-postgres` in this increment. A service that needs inbox and outbox writes in one transaction must use an SQLx-compatible outbox write through the inbox callback or migrate that composition in a separate reviewed slice; it must not pass an unrelated `tokio-postgres` connection and claim atomicity.

## Alternatives considered

### Keep `tokio-postgres`

- Benefits: no dependency or callback migration.
- Costs and risks: retains positional row handling and misses the requested common SQLx query surface.
- Reason not selected: the inbox adapter can switch coherently while preserving the portable port and transaction invariant.

### Add SQLx beside `tokio-postgres` inside the adapter

- Benefits: migrate one query at a time.
- Costs and risks: the two drivers cannot share a local transaction; inbox identity and service effects could commit independently.
- Reason not selected: atomicity is the primary contract of this adapter.

### Use SQLx `query!` macros immediately

- Benefits: build-time SQL and type checking.
- Costs and risks: requires a live schema during compilation or generated offline metadata with a regeneration and freshness gate.
- Reason not selected now: the default repository gate must stay database-free. This remains a possible follow-up once metadata generation and CI drift checks are reproducible.

## Consequences

### Positive

- Message values are bound, row mappings are named and typed, and the SQLx transaction remains one unit of work.
- The portable inbound API and persisted schema are unchanged.
- Driver-specific failures remain classified before reaching acknowledgement policy.

### Negative

- The PostgreSQL-specific callback changes from `tokio-postgres` to SQLx and must be migrated by Rust callers.
- The workspace temporarily carries both PostgreSQL drivers because the outbox adapter has not migrated.
- Runtime queries do not provide SQLx compile-time schema checking.

### Neutral or follow-up

- This decision does not add a pool, change isolation level, or migrate the outbox adapter.
- Deployed connection strings must require TLS and validate the server certificate; the checked-in local profile remains isolated. The driver is built with rustls and native trust roots, but this ADR does not define a deployment credential profile.

## Compatibility and migration

There is no database or event-wire migration. Rust callers of `PostgresInbox` and `PostgresTransactionalMessageHandler` change their concrete connection and transaction types in the same repository increment. PostgreSQL conformance and event-spine tests are updated together. An outbox composition that must share this transaction needs a separately reviewed SQLx-compatible write path; no cross-driver atomicity is implied.

## Security and operations

Untrusted values use SQLx bind parameters. The only dynamically assembled SQL in tests consists of process-derived schema identifiers and is explicitly audited; runtime adapter queries are static. `InboxError` keeps driver diagnostics out of public formatting. Least-privilege schema ownership, retention, quarantine, and replay authorization remain governed by the inbox guide and ADR-0006.

The [0.9.0 release](https://github.com/transact-rs/sqlx/discussions/4271) and continuing upstream activity provide a current maintenance signal. Crate metadata declares Rust 1.94, below this workspace's Rust 1.98.1. The dependency is pinned, dual MIT/Apache-2.0 licensed, restricted to the infrastructure adapter plus test composition, and replaceable behind `InboundMessageStore`. Supply-chain policy and SBOM generation must review the resulting locked dependency graph.

## Validation

- Run repository and architecture sensors, formatting, workspace Clippy, and all database-free tests.
- Run the opt-in PostgreSQL inbox, error-classification, and handler conformance tests in the local-platform CI job.
- Run event-spine restart conformance against the same database schema with SQLx inbox and existing `tokio-postgres` outbox connections.
- Treat PostgreSQL CI and supply-chain job results as merge gates; a compiling test is not a passing database test.
