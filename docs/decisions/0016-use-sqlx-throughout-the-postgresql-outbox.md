# ADR-0016: Use SQLx throughout the PostgreSQL outbox

- Status: Accepted
- Date: 2026-10-06
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes: ADR-0015
- Superseded by:

## TL;DR

- Use SQLx for every PostgreSQL outbox operation, including enqueue, leasing, replay, and relay transactions.
- Expose one `PostgresOutbox::enqueue` method that accepts the caller's mutable SQLx transaction; remove the temporary `enqueue_sqlx` and `tokio-postgres` path.
- Keep database-independent default builds. Compile-check outbox enqueue in the next M02 task using reproducible offline metadata and a CI freshness check, then extend the same workflow to other queries.

## Context

[ADR-0015](0015-allow-atomic-sqlx-outbox-enqueue.md) added a second enqueue entry point to close an immediate inbox/outbox atomicity gap. That temporary design left two drivers, two transaction types, and two enqueue implementations in the outbox adapter. Both call paths had to preserve exact-byte idempotency, while leasing, replay, and the relay still used `tokio-postgres`. The refactor now has enough conformance coverage to migrate the whole adapter in one reviewable change rather than maintain parallel driver paths.

## Decision

All outbox storage methods accept a mutable `sqlx::Transaction<Postgres>`. `PostgresOutboxRelay` owns a mutable `sqlx::PgConnection`, begins and commits a short SQLx transaction for each claim or outcome, and never holds it during broker publication. `PostgresOutbox::enqueue` is the sole enqueue API for standalone producers and inbound callbacks. The callback passes the transaction it received from the inbox adapter, preserving one local commit for inbox identity, service state, and outbound intent.

SQLx queries bind untrusted values. The adapter maps claimed records and replay audit records through private typed rows, and PostgreSQL returns only a Boolean when comparing immutable content after an enqueue conflict. It classifies deterministic SQLx schema and codec failures as storage invariants rather than retryable outages. The stored schema, exact envelope bytes, lease predicates, replay audit semantics, and portable application ports do not change.

## Alternatives considered

### Keep both PostgreSQL drivers

- Benefits: no migration of relay and existing callers.
- Costs and risks: duplicate enqueue APIs and transaction types invite accidental split commits and future semantic drift.
- Reason not selected: the dual path was explicitly temporary, and the complete adapter can now move together.

### Change only the relay but retain both enqueue methods

- Benefits: smaller immediate Rust API change.
- Costs and risks: leaves the same transaction ambiguity at the producer boundary.
- Reason not selected: one driver and one enqueue method are the intended outcome of this refactor.

### Require a live database for SQLx `query!` now

- Benefits: compile-time validation of SQL and bindings against one running schema.
- Costs and risks: makes the default contributor build depend on a local service and connection configuration, violating the credential-free build contract.
- Reason not selected: the next M02 task will compile-check outbox enqueue with checked-in `.sqlx` metadata generated from migrations and verify freshness in database-backed CI without a default build-time service requirement.

## Consequences

### Positive

- One outbox driver and one enqueue API preserve the shared transaction boundary without duplicate implementations.
- Typed SQLx row mappings replace positional `tokio-postgres` extraction for claims and replay evidence.
- Relay claim, publication outcome, retry, quarantine, and replay continue to use short transactions with the existing lease-generation fence.

### Negative

- Rust callers of the concrete outbox adapter must switch to mutable SQLx transactions and `PgConnection`; `enqueue_sqlx` is removed.
- Runtime SQLx queries are still validated by PostgreSQL conformance tests rather than compile-time query metadata until the scheduled next task.

### Neutral or follow-up

- The portable messaging and relay ports, migrations, persisted records, event bytes, and at-least-once delivery semantics remain unchanged.
- Standalone M08 experiments have their own dependency graphs and are not part of the outbox adapter migration.

## Compatibility and migration

This is a breaking Rust API change for direct users of `PostgresOutbox` and `PostgresOutboxRelay`, but it does not alter database or wire formats. Update all in-workspace callers and conformance tests together. A deployed old relay and new relay may read the same schema during a rolling change because their lease-generation predicates and stored fields are unchanged; quiescing old workers before replacing them keeps operational attribution clear. Rollback restores the prior binary without a database down-migration.

## Security and operations

Every untrusted value remains bound to a SQL parameter. SQLx adapter errors retain internal source chains but expose bounded public categories. The caller continues to own commit or rollback of `enqueue`; a relay owns only its short claim and outcome transactions. No external publication occurs inside a database transaction, and no new authority or table access is granted. The default build receives no checked-in database credential or service URL.

## Validation

- Run repository and architecture sensors, workspace formatting, Clippy, and database-free tests with the locked dependency graph.
- Run the outbox identity, lease-generation, quarantine, replay, relay, inbound atomicity, and event-spine PostgreSQL conformance tests in the local-platform CI profile.
- Require that active workspace manifests and outbox code have no `tokio-postgres` dependency, `enqueue_sqlx` entry point, or outbox path that opens a second connection for an inbound transaction. Historical ADRs and the standalone DBOS experiment retain their own dependency history.
- Execute the next M02 task for compile-checked outbox enqueue, checked-in SQLx offline metadata, and a database-backed freshness gate before claiming compile-time query verification.
