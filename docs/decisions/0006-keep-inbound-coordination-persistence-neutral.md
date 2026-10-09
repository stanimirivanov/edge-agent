# ADR-0006: Keep inbound coordination persistence-neutral

- Status: Accepted
- Date: 2026-09-29
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- The one-delivery coordinator depends on a semantic `InboundMessageStore`
  port, not on PostgreSQL clients, transactions, or inbox repository types.
- A store implementation atomically combines inbox deduplication with the
  first delivery's service-owned transition and reports `Applied` or
  `Duplicate` only after commit.
- The PostgreSQL adapter owns transaction begin, commit, and rollback. It may
  expose `tokio_postgres::Transaction` only through its adapter-specific
  callback for service-owned SQL work.
- Broker acknowledgement follows the store's committed result. Unavailable or
  ambiguous storage outcomes request redelivery, preserving at-least-once
  recovery.
- Quarantine is a second semantic store operation that must commit exact,
  bounded evidence before terminal broker settlement.

## Context

The inbound coordinator owns portable application policy: decode and route an
untrusted delivery, distinguish duplicate from first delivery, classify retry
and terminal failures, persist quarantine evidence, and settle the broker
delivery in the correct order. That policy does not require PostgreSQL.

Atomicity does require a concrete persistence mechanism. For a first delivery,
the consumer-scoped inbox identity, service-owned domain transition, and any
transactional outbox records must commit or roll back together. The initial
implementation achieved this by passing a `tokio_postgres::Client` into the
coordinator and a `tokio_postgres::Transaction` into its handler extension
point. This preserved correctness but coupled application orchestration to one
database driver and forced credentialed PostgreSQL integration tests to
exercise policy that can be tested without infrastructure.

A generic CRUD repository would hide the transaction without expressing the
business invariant. Splitting inbox insertion and domain work into independent
portable calls would be worse: no coordinator ordering can make two separately
committed operations atomic. The boundary must therefore represent the whole
atomic capability, while its adapter retains control of the concrete unit of
work.

## Decision

`edgeagent-messaging` owns the consumer-facing `InboundMessageStore` port and
its portable values. The port exposes two semantic operations:

1. `process` receives a logical consumer name, validated message registry, and
   envelope. It atomically records or verifies the inbox identity and applies
   service-owned work only for a first delivery. It returns `Applied` or
   `Duplicate` after the operation commits.
2. `quarantine` receives bounded `InboundQuarantine` evidence. It returns
   `Inserted` or `AlreadyPresent` only after that evidence commits.

The port reports stable categories rather than database errors. Contract
rejection, message-identity conflict, unavailable or ambiguous storage, storage
invariant failure, and classified service-handler failure remain distinguishable
so the coordinator can apply explicit policy. Causes stay in the Rust error
chain for redacted diagnostics; payloads and database diagnostics do not become
public error text.

The error postconditions are part of the port contract. A handler failure means
the inbox and service-owned transition rolled back. Contract, identity-conflict,
and invariant errors mean no service transition committed. An unavailable
result may represent an ambiguous commit and therefore requires redelivery of
the same immutable message. A quarantine success confirms committed evidence;
no quarantine error permits terminal broker settlement.

`edgeagent-inbox-handler` depends only on this port, message contracts,
transport settlement, and telemetry. It does not begin, commit, or roll back a
database transaction. It acknowledges an `Applied` or `Duplicate` delivery
only after the store returns the committed disposition. It requests delayed
redelivery for transient handler failures and unavailable or ambiguous storage.
It terminally settles poison evidence only after `quarantine` returns a
committed disposition. Invariant failures leave the delivery unsettled and
surface an error for operator investigation.

`edgeagent-inbox-postgres` implements the port as
`PostgresInboundMessageStore`. The adapter owns transaction begin, commit, and
rollback around the existing PostgreSQL inbox and quarantine mechanics. For a
first delivery it invokes `PostgresTransactionalMessageHandler` inside that
same transaction, preserving one atomic commit for inbox identity,
service-owned state, and transactional outbox records. A duplicate skips the
callback.

`PostgresTransactionalMessageHandler` is an infrastructure composition seam,
not a domain or persistence-neutral application port. It may expose
`tokio_postgres::Transaction` because its purpose is to compose service-owned
SQL with the PostgreSQL inbox unit of work. Implementations keep deterministic
business decisions in application or domain code, write only service-owned
tables, and perform no network calls or other effects that cannot roll back
with the transaction.

Other storage profiles may implement `InboundMessageStore` only when they can
provide equivalent atomic and idempotent semantics. The port is a capability
contract, not evidence that every database can satisfy it or that distributed
transactions are supported.

## Alternatives considered

### Keep PostgreSQL in the application coordinator

- Benefits: fewer types and direct visibility of each transaction step.
- Costs and risks: application policy depends on `tokio-postgres`; unit tests
  need database credentials; another storage profile requires forking policy.
- Reason not selected: transaction mechanics belong to the infrastructure
  adapter, while settlement and retry decisions are portable application policy.

### Introduce a generic transaction or unit-of-work abstraction

- Benefits: the coordinator could retain explicit transaction sequencing.
- Costs and risks: a lowest-common-denominator transaction leaks persistence
  mechanics, invites database-specific downcasts, and does not express the
  required inbox-plus-domain invariant.
- Reason not selected: one semantic atomic operation is smaller and makes the
  required postcondition explicit.

### Split inbox deduplication and domain handling into separate ports

- Benefits: each interface appears narrowly scoped and easy to mock.
- Costs and risks: separate calls can commit independently, allowing an inbox
  marker without its domain transition or duplicate domain work after a crash.
- Reason not selected: preserving one local atomic transition is mandatory.

### Hide SQL behind a persistence-neutral domain callback

- Benefits: no adapter-specific callback would expose a driver transaction.
- Costs and risks: independently abstracted domain repositories cannot join the
  inbox adapter's concrete transaction without leaking or duplicating the same
  unit-of-work abstraction.
- Reason not selected: the SQL callback is deliberately confined to the
  PostgreSQL adapter; application and domain policy remain driver-independent.

## Consequences

### Positive

- Retry, quarantine, and settlement policy can be tested with credential-free
  in-memory fakes.
- PostgreSQL types no longer cross the application-facing inbound boundary.
- The port states the atomic business capability instead of mirroring inbox
  tables or CRUD operations.
- Alternative adapters can be assessed against explicit commit, duplicate, and
  failure semantics.

### Negative

- The PostgreSQL composition root and service SQL callback depend on the
  PostgreSQL adapter API.
- A store implementation is responsible for a larger semantic operation and
  requires integration tests to prove transaction behavior.
- Portable error categories intentionally lose database-specific policy detail;
  adapter causes remain available only through the internal error chain.

### Neutral or follow-up

- This decision does not add a consumer loop, connection pool, concurrency
  model, distributed transaction, or service-specific domain handler.
- Inbox and quarantine schemas, message wire contracts, and broker settlement
  meanings do not change.
- Service-specific handlers arrive with their owning vertical slices and must
  pass the same adapter conformance behavior.

## Compatibility and migration

This change has no persisted-data or wire-format migration. Existing inbox,
quarantine, and replay audit rows retain their meaning.

The Rust composition API changes: callers construct a concrete inbound store
and pass it to `handle_once`; PostgreSQL service callbacks implement
`PostgresTransactionalMessageHandler` in the adapter boundary. This workspace
has no published stable Rust API, so consumers migrate in the same change. A
future non-PostgreSQL adapter must pass equivalent duplicate, rollback, commit
ambiguity, quarantine, and acknowledgement-ordering tests before adoption.

## Security and operations

- `InboundQuarantine` validates delivery identity, subject, attempt, payload
  size, and failure code before adapter work; exact untrusted bytes remain data,
  never SQL or telemetry text.
- The PostgreSQL callback receives a powerful transaction handle. Composition
  roots must provide least-privilege credentials, write only service-owned
  tables, and keep authorization and input validation outside raw SQL strings.
- External network calls are prohibited inside the transaction because their
  effects cannot roll back atomically and would extend lock duration.
- Commit ambiguity is classified as unavailable. Redelivery resolves the state
  through durable inbox or quarantine identity instead of assuming success.
- Metrics preserve bounded stage and outcome dimensions; adapter causes use
  redacted operational logs and traces.

**2026-10-08 refinement:** [ADR-0018](0018-use-validated-message-failure-codes.md)
assigns delivery identity, subject, and attempt validation to
`DeliveryMetadata`, and failure-code validation to `FailureCode`.
`InboundQuarantine` still checks payload size before adapter work. The
pre-adapter evidence bound and quarantine-before-settlement decision remain.

## Validation

- Architecture checks reject PostgreSQL adapter or driver dependencies from
  normal `edgeagent-inbox-handler` dependencies.
- Credential-free coordinator tests cover applied, duplicate, retry,
  quarantine, unavailable-store, invariant, and settlement behavior through a
  fake `InboundMessageStore`.
- PostgreSQL integration tests prove first-delivery atomicity, duplicate
  suppression, handler rollback, quarantine idempotency, commit-before-settle,
  and recovery after acknowledgement ambiguity.
- Event-spine conformance continues to compose the production PostgreSQL and
  JetStream adapters across abandoned delivery and restart scenarios.
