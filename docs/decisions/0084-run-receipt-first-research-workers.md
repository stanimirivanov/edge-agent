# ADR-0084: Run receipt-first Research workers

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Recover durable Research completion before reading evidence or evaluating again.
- Obtain bytes through an explicitly configured MarketData owner process, never peer tables.
- ACK only after completion commit or confirmed quarantine; relay exact retained events.

## Context

M04 has deterministic evaluation, durable Research completion and MarketData evidence
handoff. Research needs an activated bounded consumer with an authorized read path.
References and source URI claims cannot grant access to MarketData's private state.

## Decision

Add a consumer-owned EvidenceLoader port and receipt-first application orchestration.
Accept the MarketData evidence-decided event in the completion registry and validate
its original request correlation. Preserve owner abstention reasons. For ready
results, load authorized exact bytes, construct pure frozen evidence and run existing
minimum-evidence, momentum, ranking and canonical rendering before the existing
completion/inbox/outbox transaction. Unavailable reads leave completion absent;
denied/corrupt/unsupported evidence becomes a typed abstention.

Use the explicit offline MarketData owner-read command as the minimum read interface.
The owner binds the exact retained correlated publication, acquisition and operator
scope, then uses current authority and RetainedEvidenceStorage before exporting
canonical daily-v1 bytes. The Research adapter uses fixed operator paths, isolated
child environment, bounded stdin/stdout and timeout, exact descriptor/digest checks
and independently configured implementation/configuration/freshness/expiry bounds.
No client path, provider, URL or command is interpreted. No peer database is read.

Research exposes explicit local stdin, finite JetStream worker and relay modes;
descriptor commands remain unchanged. Poison input is durably quarantined before
termination. Unavailable reads retry three times; exhaustion requires receipt recovery
before quarantine, so ambiguous commit success can still be acknowledged. Publication
uses the established leased outbox relay and first retained bytes.

## Alternatives considered

Direct owner table/object reads bypass authority and ownership. Reading a cached
reference as permission ignores revocation/retirement. Recomputing after a completed
request changes original causation or requires evidence already erased. A new
workflow engine, provider, transport or persistence abstraction is unnecessary.

## Consequences

The trusted local profile supports the existing single-instrument daily-v1 owner
export. It is not a public authentication surface. Policy revisions and limits are
operator-reviewed explicit settings; payload revisions cannot select policy.
Calendar/mapping lineage derives from exact owner references, session census from
accepted bars, and availability includes capture/actual receipt/reference knowledge.
Submillisecond values fail instead of being rounded into earlier availability.
Research must still abstain when point-in-time policy rejects otherwise readable
evidence. Historical artifacts retain explicit expiry, even if already expired.

## Compatibility and migration

No new table or schema rewrite is required: apply existing Research/inbox/outbox
migrations in a separately prepared private schema before activation. Existing
completion bytes and Research v1 schemas remain unchanged. Old PR7 direct evaluation
commands have no authorized descriptor and drain to missing-entitlement abstention;
they never trigger arbitrary reads. New requests consume MarketData evidence events.
Stop intake before rollback and retain authoritative completion/inbox/outbox state.

## Security and operations

Configure loopback databases/broker and independent owner/Research scopes and paths.
The owner-read process returns 77 for denied/absent access, 65 for invalid input and
75 for storage unavailability. It exports only a current authorized exact object;
Research does not inherit its database pool or filesystem access. The adapter caps
output at four MiB, kills/reaps failed/timed-out children and suppresses private
stderr. Scope is checked before receipt recovery. No credentials, object bytes or
SQL errors are printed by Research. No provider/model/live execution is activated.

## Validation

Protect conversion/descriptor tests and real PostgreSQL worker process recovery,
corrupt/denied/unavailable reads and owner authorization regressions. Real JetStream
proofs cover ACK after commit, repeated delivery/restart, exact event relay, poison
correlation and retry exhaustion without completion. Existing Research persistence
proofs retain rollback/contention coverage. Canonical SQLx and supply-chain gates
remain required where their pinned tooling is available.
