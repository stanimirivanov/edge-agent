# ADR-0082: Admit research through a durable local Gateway

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Explicit loopback serving binds requests to one trusted local operator scope.
- Return HTTP 202 only after committing admission and evaluation outbox intent.
- Same scoped request retries retain the original command; changed content conflicts.

## Context

Research completion is durable, but clients need an ingress boundary that survives
lost acknowledgements without creating duplicate evaluation commands. No public
identity provider or deployment profile has been approved for this milestone.

## Decision

Add `edgeagent-gateway serve`, explicitly configured with loopback listener/database,
private prepared schema and one operator scope. All local processes in this profile
are trusted to act as that operator. Client scope must match configuration; it is
never an authority claim. Reject browser-origin requests and nonlocal Host names.
No CORS, reverse proxy or public exposure is supported.

POST the existing v1 Request DTO to `/v1/research-requests`. Bound input before
JSON parsing. Canonical record bytes establish equality under the scoped request
ID. In one READ COMMITTED transaction insert immutable Gateway admission and
existing Research-owned EVALUATE outbox command. Use the request digest for message
identity and idempotency, scoped subject/partition and database UTC admission time.
Return 202 only after commit. Retry identical intent to recover success; changed
intent returns 409. Retained admission verifies exact command/outbox integrity.

## Alternatives considered

Reusing Axum, SQLx and the established outbox avoids another framework or queue.
Responding before commit can lose accepted work. A memory-only idempotency key
cannot survive restart. Public authentication is deferred rather than simulated
with a caller-supplied header or scope. Evaluation remains an asynchronous concern.

## Consequences

Acceptance means durable intent, not completed research. Preserve original command
bytes/time on retries. Storage/commit uncertainty returns 503: retry the same ID,
including when a timeout may follow successful commit. Retain outbox integrity
rows for the admission lifetime. No repair, relay, query or provider operation runs.

## Compatibility and migration

Descriptor commands remain unchanged. Apply the additive Gateway migration with
existing outbox migrations in a separate Gateway-owned schema before serving;
startup never performs DDL. Rollback stops serving and preserves durable rows.
No existing research schema or request bytes change. Later public exposure needs
reviewed authentication, authorization and transport security before activation.

## Security and operations

Listener and database configuration require loopback; database URL overrides are
refused. This profile cannot distinguish local users; use only on a trusted host.
Reject Origin, foreign scope and nonlocal Host. No forwarded identity is trusted.
Use a runtime role without DDL, TRUNCATE, trigger-disable or admission mutation
rights. Transactions use bounded locks/statements and HTTP admission has a deadline.
Diagnostic responses contain no payload, database URL or driver errors.

## Validation

Protect HTTP test discovery and real PostgreSQL proofs for retries/restart,
content conflicts, concurrency, separate scopes, statement/deferred-commit rollback
and retained-integrity refusal. Extend canonical SQLx assembly and cached queries.
