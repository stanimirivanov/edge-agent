# ADR-0026: Compose durable acceptance in independent processes

- Status: Accepted
- Date: 2026-10-17
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Add a finite conformance-only composition root with producer, relay and
  consumer roles, executed as separate OS processes.
- Commit acceptance/outbox together; commit inbox/probe/outbound fact together.
- Keep private service schemas, restricted runtime logins, create-only broker
  provisioning, and publication outside SQL transactions.
- Protect actual process proofs with the canonical T3 inventory. This is not
  full crash, wire-acknowledgement-loss or production lifecycle qualification.

## Context

ADR-0024 defines typed synthetic contracts; ADR-0025 defines bounded topology
and permissions. Earlier adapter proofs run components within one test process.
They do not prove that independently launched composition roots can recover
durable intent after their predecessors exit, or that runtime credentials
respect service-owned persistence boundaries.

## Decision

Extend the existing tooling package, not product service shells. Its
`conformance-worker` binary accepts one explicit role, reports readiness after
dependency connection, waits for one bounded parent action and performs one
operation. Configuration arrives through child environment variables, not
credential-bearing command-line arguments. The fixture supplies only its
service's database login and generated NATS runtime identity.

The producer commits an immutable acceptance identity and canonical command
outbox intent in one SQLx transaction. Re-entry uses identical identity/content.
The relay calls the production recovery-aware `relay_once` outside caller-owned
transactions. The consumer uses the production JetStream adapter,
`handle_once`, PostgreSQL atomic inbound store, and a service-owned SQL callback.
That callback writes an initial probe revision and enqueues a typed causal fact
inside the inbox transaction. A separate consumer-owned relay publishes the fact.
The coordinator acknowledges only after committed processing.

The callback deliberately supports one initial synthetic revision. Semantic
business-key deduplication, predecessor ordering and competing admission belong
to the next slice, not hidden assumptions about broker FIFO. Fixed probe/message
identities are isolated by fresh schemas and an exclusive broker per scenario.

Checked SQLx macros and committed offline metadata cover service-owned writes.
The metadata action also assembles the synthetic fixture schema in its isolated
metadata database. Dynamic schema/grant/inspection SQL remains test-only, with
identifiers derived solely from process ID, an atomic counter and fixed suffixes.

## Alternatives considered

- Keep in-process composition: cannot prove OS-process boundaries or handoffs.
- Build product research/gateway workers now: invents application behavior
  outside M02 and makes synthetic evidence appear product-complete.
- Add a generic worker framework: no second production use justifies it.
- Share an unrestricted database login: cannot demonstrate private write ownership.
- Add callback hooks to storage adapters: contaminates reusable APIs with fixture
  coordination and hides actual transaction boundaries.

## Consequences

The tooling package gains explicit normal dependencies on application ports and
production adapters; both architecture sensors record them. This does not
authorize those dependencies in domain/contract crates, add an OCI deployable,
or change production schemas/wire contracts. Runtime database roles differ from
the administrator that creates/migrates/cleans fixture resources. Relay logins
can select/update only their own outbox; producer/consumer logins can perform
their own insert/identity-check work, not migration or peer writes.

## Compatibility and migration

No published message or adapter API changes. Apply adapter migrations plus the
owning synthetic table in each disposable schema. Metadata generation applies
both synthetic tables in public solely to describe checked queries. Never apply
the conformance schema to a product deployment. Removing this tooling requires
no production down-migration; retain evidence until the isolated proof finishes.

## Security and operations

**[Prerequisite from M10]** Readiness, deadlines, finite intake and shutdown are
limited tooling requirements. Workers use two runtime threads, one database
connection, one delivery/claim and bounded I/O. The parent waits on a readiness
line, sends one action byte, closes stdin and awaits exit; deadlines kill/reap
owned children. Worker timeout/error text never prints database URLs, payloads
or source chains. Tokio runtime shutdown is bounded even if stdin is abandoned.

The proof creates only exclusively owned schemas/logins and uses the existing
owned authenticated broker fixture. Ordinary error, panic and deadline paths
attempt bounded cleanup. Process abort remains covered by disposable CI
teardown; there is no promise of distributed rollback. Cancellation/failed
commit confirmation is unresolved and must be retried with immutable identity,
never treated as confirmed success or compensated by deleting evidence.

Public `fixture-only` credentials identify local synthetic resources, not
production secrets. PostgreSQL role creation requires an isolated administrative
test endpoint. Runtime identities receive no peer-schema or replay-audit access.
No model calls, market data, live-order path, cloud deployment or GitOps changes.

## Validation

The protected `event-spine-processes` T3 suite launches actual children and checks
producer re-entry, committed publication, consumer transition/fact intent,
second transport publication without broker deduplication, typed causal fact,
empty command stream/no pending ACK, rollback of both transaction boundaries,
redelivery after rollback and denied peer writes/DDL. Census mutation tests reject
missing, renamed, duplicate or additional ignored cases. Default tests compile
the binary and proof without services; only executing T3 proves runtime behavior.
See the [process guide](../development/process-acceptance.md) for commands and
evidence limits. Full crash barriers, broker restart and wire ACK loss remain
slice 7; telemetry export and semantic ordering remain separate slices.
