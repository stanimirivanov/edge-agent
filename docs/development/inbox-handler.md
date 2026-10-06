# Transactional inbox handler

## TL;DR

- `edgeagent-inbox-handler` applies portable one-delivery policy through
  `InboundMessageStore`; it has no database-driver types in its application
  boundary.
- `process` is one semantic atomic operation: deduplicate the delivery and, for
  a first delivery, commit service-owned state and outbox records with the
  inbox identity.
- `PostgresInboundMessageStore` owns PostgreSQL transaction begin, commit, and
  rollback. Only its adapter-specific service callback receives a
  mutable `sqlx::Transaction<Postgres>`.
- A committed first delivery or identical duplicate is acknowledged. Transient
  failures request bounded delayed redelivery.
- Permanent or exhausted failures commit exact quarantine evidence before
  terminal settlement; storage or settlement ambiguity never becomes success.

## Application boundary

`edgeagent-messaging` owns the portable inbound values and
`InboundMessageStore` port required by the coordinator. This is a semantic
capability, not an inbox-table repository:

- `process(consumer_name, registry, envelope)` atomically deduplicates the
  message and applies service-owned work for a first delivery. It returns
  `Applied` or `Duplicate` only after commit.
- `quarantine(consumer_name, evidence)` atomically retains bounded terminal
  evidence. It returns `Inserted` or `AlreadyPresent` only after commit.

The port exposes stable contract, identity-conflict, availability, and
invariant error categories. Adapter causes remain in the error chain for
redacted diagnostics, but database diagnostics and payload content do not enter
public errors or metric dimensions.

`edgeagent-inbox-handler::handle_once` receives a mutable store, message
registry, handler policy, and owned transport delivery. It owns decoding,
routing, retry classification, quarantine ordering, settlement, and telemetry.
It does not receive a database client or transaction and does not own an intake
loop, connection lifecycle, parallelism, or service-specific domain policy.

The crate root is the public façade. `coordinator` owns decoding, routing, and
the atomic processing decision. `resolution` owns failure classification,
including the store contract, identity-conflict, availability, and invariant
branches, plus quarantine-before-settlement and the one-shot delivery actions.
`policy` validates consumer configuration and failure codes; `retry` calculates
deterministic backoff; `outcome` defines confirmed results and classified
message failures; and `error` defines bounded coordinator failures.
`observability` owns one private `SpineRecorder` for approved correlation
context, stage timing, and classified event-spine signals. It starts the
handling timer after decoding and records persistence and acknowledgement only
after their futures return. Every atomic `process` result, including a handler
failure that rolls back, receives one classified persistence signal before
resolution. A completed quarantine-store result likewise produces one
persistence signal, with both inserted and already-present evidence classified
as quarantined. These modules are private, so callers continue to use the same
crate-root types and `handle_once` function.

One private settlement path records the confirmed broker result for acknowledge,
retry, and terminal quarantine; successful applied or duplicate outcomes derive
from the committed `InboxDisposition` rather than a second local enum.

The durable rationale is recorded in
[ADR-0006](../decisions/0006-keep-inbound-coordination-persistence-neutral.md);
the concrete SQLx adapter choice is recorded in
[ADR-0014](../decisions/0014-use-sqlx-for-postgresql-inbox.md).

## PostgreSQL composition

`edgeagent-inbox-postgres::PostgresInboundMessageStore` implements the portable
port. A composition root constructs it from a mutable `sqlx::PgConnection`
and a `PostgresTransactionalMessageHandler` implementation, then passes the
store to `handle_once`.

The adapter begins the transaction, records or verifies the inbox identity,
invokes the PostgreSQL callback only for a first delivery, and commits or rolls
back. The callback receives the validated `MessageEnvelope` and the adapter's
mutable `sqlx::Transaction<Postgres>`. It may update service-owned tables and must
enqueue any outbox messages through `PostgresOutbox::enqueue_sqlx` on that same
transaction. The callback must delegate deterministic business
decisions to application or domain code, must not write another service's
tables, and must not perform network calls or other external effects that
cannot roll back atomically.

The PostgreSQL callback is deliberately infrastructure-specific. Driver types
remain inside the PostgreSQL adapter and composition layer; they do not become
domain types or leak back into portable coordinator policy.

## One-delivery control flow

`handle_once` applies this order:

1. Decode the raw structured CloudEvents bytes and validate them against the
   process `MessageRegistry`.
2. For malformed, unsupported, or misrouted input, call `quarantine`; request
   terminal settlement only after the evidence commits.
3. For a valid envelope, call `process` once.
4. The store begins its concrete transaction and records or verifies the
   consumer-scoped inbox identity.
5. On a first delivery, the store invokes service-owned work and commits the
   inbox identity, domain transition, and any outbox records atomically.
6. On an identical duplicate, the store skips service-owned work and returns a
   committed `Duplicate` disposition.
7. The coordinator acknowledges `Applied` or `Duplicate` only after the store
   returns success.
8. A transient service failure rolls back and requests deterministic delayed
   redelivery while the attempt budget remains.
9. A permanent or exhausted service failure rolls back the processing
   transaction, commits quarantine evidence through a separate store operation,
   and then requests terminal settlement.

The coordinator never assumes a failed commit rolled back. An unavailable or
ambiguous store result requests redelivery. Durable inbox and quarantine
identity resolve the outcome on the next attempt.

## Retry and terminal policy

`HandlerPolicy` requires a stable logical consumer name, 1–100 delivery
attempts, and base/maximum delays from one millisecond through 24 hours. Retry
delay uses capped exponential backoff with deterministic message-key jitter.
Every replica therefore makes the same decision for the same delivery without
synchronized randomness. Whole-millisecond policies retain their established
delay sequence; finer configured durations retain nanosecond precision while
respecting the portable one-millisecond minimum. The broker's delivery-attempt
counter is the budget, so transport and storage redeliveries count toward the
same limit.

| Failure | Resolution |
| --- | --- |
| Malformed envelope or invalid route | Commit `envelope_invalid` or `routing_invalid`, then terminally settle |
| Inbox message-identity conflict | Commit `message_identity_conflict`, then terminally settle |
| Transient service handler below attempt limit | Roll back and request delayed redelivery |
| Permanent service handler failure | Roll back, commit its bounded reason code, then terminally settle |
| Transient service handler at attempt limit | Treat as terminal and retain its bounded reason code |
| Store unavailable or commit outcome ambiguous | Request redelivery; never terminally settle without committed quarantine evidence |
| Store invariant or quarantine identity conflict | Return an error, leave delivery unsettled, and fail closed for operator investigation |
| Settlement confirmation failure | Return `Settlement`; rely on inbox or quarantine idempotency when redelivered |

Handler reason codes are static lowercase ASCII tokens of at most 64 bytes.
Portable quarantine evidence also bounds delivery key, subject, attempt, and
payload before adapter work.

## Crash and acknowledgement safety

A crash before the processing transaction commits leaves no inbox record or
domain effect. A crash after commit but before acknowledgement causes
redelivery; `process` returns `Duplicate`, so service-owned work does not run
again. A crash after quarantine commit but before terminal-settlement
confirmation causes redelivery; `quarantine` returns `AlreadyPresent` before
terminal settlement is retried.

PostgreSQL commit failures are treated as ambiguous because the server may have
committed without returning confirmation. Redelivery is safe in either case:
committed work is discovered through retained identity, while uncommitted work
can execute again.

## Verification

Credential-free tests use an in-memory `InboundMessageStore` to exercise
portable applied, duplicate, retry, quarantine, unavailable-store, invariant,
and settlement policy without PostgreSQL. A stateful quarantine test store
verifies that lost terminal confirmation is followed by `AlreadyPresent` on
identical redelivery, while conflicting bytes under the same delivery identity
fail closed without settlement. It also models an ambiguous quarantine commit:
the first attempt requests retry, and redelivery discovers retained evidence
before terminal settlement. Tests verify that lost retry confirmation never
becomes a confirmed outcome. Adapter integration remains responsible for
proving the concrete transaction boundary.

The isolated PostgreSQL conformance test runs with:

```text
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-inbox-handler --test postgres_handler -- --ignored --exact coordinator_preserves_commit_retry_and_quarantine_ordering
```

It composes `PostgresInboundMessageStore` with the coordinator and proves
first-delivery commit, duplicate suppression, transient rollback and retry,
poison-message quarantine, permanent-handler quarantine, lost terminal
confirmation recovery, and acknowledgement-loss recovery. Run it only against
an isolated development or CI database.

The [event-spine recovery conformance](event-spine-conformance.md) additionally
composes the coordinator with the production PostgreSQL and JetStream adapters,
then abandons a delivery and reconnects before handling its redelivery.

## Current limitations

This increment does not provide a long-running consumer loop, connection pool,
parallelism, acknowledgement-progress extension, graceful shutdown, payload
schema generation, authorization policy, operator replay publisher, or
telemetry exporter installation. It emits bounded durable-outcome signals
through the [event-spine telemetry contract](event-spine-telemetry.md).
Service-specific handlers and their tables remain owned by their vertical
slices. Those capabilities stay in separate M02 increments because they change
lifecycle, recovery, or domain behavior.
