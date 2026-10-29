# ADR-0081: Commit research completions atomically

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Research owns immutable scoped terminal completion and its publication intent.
- Reuse existing inbox/outbox APIs in one bounded READ COMMITTED transaction.
- Recover the first committed receipt before reading evidence or recalculating.

## Context

M04 rendering produces deterministic outcome bytes but does not establish durable
completion. At-least-once delivery and concurrent workers must not create duplicate
artifacts or publish an outcome without the corresponding owner state.

## Decision

Introduce a persistence-neutral publication port and a Research PostgreSQL adapter.
A scoped request key and exact canonical request bytes establish durable idempotency.
A terminal row is request completion. Store request, outcome and exact event bytes
with the inbox delivery and existing outbox intent in one transaction. No external
call occurs inside it. Same request returns the first retained outcome; changed
request intent conflicts. Retained state verifies its outbox identity and bytes.

Add the minimal local evaluation/artifact/abstention message definitions required
for validated enqueue. Evaluation uses the existing v1 Request record, explicitly
bound to caller scope/request and content digest. Gateway/MarketData source claims
require future outer authentication. Terminal events are Research-owned with the
content digest as event ID. This does not activate topology, workers or endpoints.

## Alternatives considered

Separate inbox, artifact and publication commits permit lost effects or duplicates.
A broker-only deduplication window cannot replace durable scoped request uniqueness.
Overwriting results on retry would discard immutable first-commit evidence. A new
transaction framework or duplicate inbox/outbox implementation is unnecessary.

## Consequences

A fresh outbox intent must accompany first completion. Receipt-first recovery avoids
recalculation, preserves first causation/time and confirms new delivery identities
atomically. Commit errors may be ambiguous; callers recover by the same identity.
Exact retained outbox rows must remain available for the completion lifetime;
coordinated retention is not introduced here. Integrity failure never repairs or
recreates a publication from caller-supplied candidate bytes.

## Compatibility and migration

Use additive service-owned migration before future worker activation. Existing
research records/schemas and demonstration envelopes remain unchanged. These local
message definitions use the existing record-union schema. Gateway admission and
MarketData correlation/runtime composition remain later slices. The runtime role
has no DDL/TRUNCATE or completion update/delete authority. Rollback preserves rows
and stops intake rather than dropping owner evidence.

## Security and operations

The outer adapter authenticates callers, audiences and producers and authorizes
current evidence use. A source URI or digest is not authorization. Transactions
set isolation and finite lock/statement timeouts. No provider, broker publication,
credential, cluster or paid-resource operation is activated. Typed causes survive
for classification without normal diagnostics formatting private payloads/SQL.

## Validation

Run real PostgreSQL proofs for contention, original receipt recovery, rollback at
statement and deferred commit boundaries, cancellation and immutable/integrity
refusal. Default acceptance protects their ignored-test census and runs protocol
binding/redaction tests. Extend canonical SQLx assembly and commit checked metadata.
