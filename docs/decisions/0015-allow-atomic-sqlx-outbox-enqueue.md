# ADR-0015: Allow atomic SQLx outbox enqueue

- Status: Accepted
- Date: 2026-10-06
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Provide a SQLx outbox enqueue entry point so an inbound service callback can write its domain state and outbound intent in the inbox transaction.
- Keep the existing `tokio-postgres` enqueue and relay path until a separate migration is justified.
- This guarantees one PostgreSQL commit for local records, not exactly-once broker publication or external effects.

## Context

[ADR-0014](0014-use-sqlx-for-postgresql-inbox.md) moved the inbound adapter's transaction to SQLx. The outbox enqueue helper still accepted only `tokio-postgres::Transaction`. A callback could therefore write service state and the inbox marker atomically, but could not use the outbox helper in the same transaction. Opening another connection would permit an inbox commit without its outbound intent, or the reverse.

The relay and existing standalone producers already use `tokio-postgres`; migrating their leasing, outcome, replay, and conformance surfaces is a separate compatibility change. The immediate need is one validated outbox write through the SQLx transaction already owned by inbound processing.

## Decision

Add `PostgresOutbox::enqueue_sqlx` for a mutable `sqlx::Transaction<Postgres>`. It shares envelope validation, canonical byte serialization, and immutable identity comparison with the existing `enqueue` path. Both methods write the same schema and return the same `EnqueueDisposition`. The inbound callback may call the SQLx method only before it returns, while the inbox adapter still owns commit or rollback.

Keep the existing `tokio-postgres` method and relay unchanged. No portable application port imports a database driver. An emitting service must apply both inbox and outbox migrations inside its service-owned schema. It must not perform external calls in the transaction.

## Alternatives considered

### Migrate the complete outbox adapter to SQLx now

- Benefits: one database driver and one enqueue API.
- Costs and risks: changes leasing, replay, relay composition, and all existing callers in a much larger compatibility slice.
- Reason not selected: the inbound atomicity gap can be closed independently and verified without changing relay behavior.

### Enqueue through a second `tokio-postgres` connection

- Benefits: no outbox API change.
- Costs and risks: separate transactions cannot commit or roll back together.
- Reason not selected: it violates the transactional outbox invariant.

## Consequences

### Positive

- A first inbound delivery can atomically commit its inbox identity, service-owned state, and outbox row.
- Both enqueue paths preserve exact-byte idempotency and fail on changed content under an existing identity.

### Negative

- The outbox adapter temporarily carries both PostgreSQL drivers and two driver-specific enqueue entry points.
- The SQLx query is checked by PostgreSQL conformance tests, not by compile-time `query!` metadata.

### Neutral or follow-up

- A later whole-adapter SQLx migration may remove the old driver and consolidate the public API after its relay and replay behavior is proven.
- Broker delivery remains at least once; downstream consumers still require durable deduplication.

## Compatibility and migration

The new method is additive. Existing Rust callers, SQL migrations, stored bytes, and message wire format remain unchanged. A new inbound callback that emits an event calls `enqueue_sqlx` with its supplied transaction. Rollback removes all local writes; a commit followed by lost broker acknowledgement is resolved by stable outbox and inbox identities, not by a claim of exactly-once delivery.

## Security and operations

Both methods bind untrusted message values and derive the subject from a validated message definition. Public outbox errors redact driver causes in `Display` and `Debug` while retaining the source chain for controlled diagnostics. Runtime identities need write access only to their service-owned inbox, domain, and outbox tables. Relay publication and its lease-fenced outcome transactions remain outside inbound work.

## Validation

- A PostgreSQL integration test fails after writing service state and enqueueing, then proves all three local records rolled back.
- Redelivery commits one inbox marker, one service transition, and one outbox row; a duplicate skips the callback.
- Direct SQLx enqueue returns `AlreadyPresent` for identical bytes and `MessageIdentityConflict` for changed content.
- Run repository and architecture sensors, workspace formatting, Clippy, tests, and the local-platform PostgreSQL smoke job.
