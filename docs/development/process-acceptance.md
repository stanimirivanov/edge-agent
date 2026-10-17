# Multi-process durable acceptance

## TL;DR

- `cargo xtask verify --check event-spine-processes` launches independent
  producer, relay and consumer OS processes against isolated PostgreSQL schemas
  and an automatically owned authenticated NATS broker.
- Set `EDGEAGENT_POSTGRES_URL` to an isolated administrative test database.
  Docker provisions the broker automatically; alternatively
  `EDGEAGENT_NATS_SERVER` names the absolute pinned 2.14.7 native executable.
- Acceptance/outbox and inbox/probe/fact intent each share one commit. External
  publication occurs only through a separate production relay invocation.
- This proves finite process composition and duplicate/rollback behavior, not
  complete crash qualification, model durability or production service readiness.

## Composition and ownership

The `conformance-worker` binary belongs to the tooling-only
`edgeagent-event-spine-conformance` package. It is not an image/deployable and
does not alter product shells. [ADR-0026](../decisions/0026-compose-durable-acceptance-in-independent-processes.md)
records ownership, lifecycle and compatibility decisions.

1. A fixture administrator creates fresh producer/consumer schemas, applies
   outbox migrations to both, inbox migrations to the consumer, and only the
   owning synthetic acceptance/probe table to each schema.
2. It creates four restricted logins: producer, producer relay, consumer and
   consumer relay. None owns a schema or receives migration authority. A relay
   can select/update only its outbox. Peer-schema writes and runtime DDL fail.
3. The authenticated provisioner creates registry-derived topology before any
   runtime child starts. Runtime broker permissions come from the same plan.
4. A producer child connects, reports `ready`, waits for one parent action byte,
   writes acceptance plus command outbox, commits and exits `accepted`.
5. A fresh producer-relay child claims/publishes/records one intent and exits
   `published`; it holds no database transaction while publishing.
6. A consumer child receives one command, validates it, atomically writes inbox,
   initial revision/value and fact intent, commits, confirms ACK and exits
   `applied`. A committed duplicate exits `duplicate` without invoking the callback.
7. A consumer-relay child publishes the fact. The audit subscriber decodes the
   typed payload and verifies correlation, causation, revision and value.

Each child receives its runtime database URL through its private environment,
never process arguments. Only schema identifiers beginning `m02_` and containing
bounded lowercase ASCII/digits/underscores enter connection search-path options.
Broker namespace and runtime component are validated against the topology plan.
Producer requires no broker connection. There is no automatic topology mutation
or schema creation inside a worker.

## Proof scenarios

`independent_processes_commit_acceptance_transition_and_fact_once` starts fresh
processes for every operation, including identical producer re-entry and idle
relay re-entry. It verifies one acceptance, published command, inbox, probe and
fact intent. It republishes identical command bytes without `Nats-Msg-Id` so the
second transport delivery cannot be hidden by broker publication deduplication.
The new consumer returns `duplicate`, one transition/fact remains, and command
ACK state is empty after commit. Actual SQLSTATE 42501 proves peer-write and DDL
denials, not an inference from a timeout.

`process_rollbacks_leave_no_partial_acceptance_or_inbound_fact` asks the tooling
producer to roll back after enqueue, proving neither row survives. It then
accepts/publishes normally. A tooling callback returns a transient failure after
inbox, state and fact enqueue, proving all three roll back before retry is
confirmed. A fresh consumer receives the retained command and commits normally.
These synthetic controls remain outside production adapter APIs.

## Lifecycle and failure contract

**[Prerequisite from M10]** Each child handles one operation only: two runtime
threads, one connection, one delivery or claim. Readiness follows actual
connection, not elapsed time. The parent starts work with one byte (`c` commit,
`r` fixture rollback), closes stdin, then checks a bounded result and exit.
Children have a 30-second total deadline; the proof has a tighter 15-second child
budget and 90-second scenario budget. SQL statements are limited to 5 seconds,
locks to 2 seconds, broker operations/receive to 5 seconds. Runtime shutdown is
bounded even if a parent abandons stdin. No arbitrary sleep coordinates work.

The parent kills/reaps owned children on ordinary errors/deadlines; kill-on-drop
is a cancellation backstop. Owned database and broker fixtures attempt bounded
cleanup on errors, panics and deadlines. Disposable CI teardown handles process
abort. Unknown or cancelled outcomes are not confirmed rollback/success; retained
identity and outbox/inbox evidence determine safe re-entry. Never rerun with
new message IDs to conceal an uncertain result.

Worker errors intentionally omit source chains and payloads. A failed child
returns unresolved failure, not a fabricated durable success. Tests distinguish
explicit rollback, committed success, duplicate and idle outcomes. Neither child
exit nor broker health alone constitutes transaction evidence.

## Verification and metadata

Default `cargo xtask verify` compiles the worker and discovers the ignored T3
suite; it does not execute service-backed proofs. The canonical local-platform
profile runs the protected suite in existing CI with isolated PostgreSQL and
automatically created NATS resources. Missing prerequisites fail, never skip.

```text
cargo xtask verify --check event-spine-processes
```

Both service-owned insert statements use checked `sqlx::query!` and committed
offline metadata. The [metadata action](sqlx-metadata.md) also applies
`crates/event-spine-conformance/fixtures/process-schema.sql` in its authorized
metadata database before preparing all targets. Test-only schema/grant/inspection
queries remain dynamic; no untrusted message fields enter SQL identifiers.

## Limits and follow-ups

This is a single initial synthetic probe (revision zero to one), not product
research/order behavior. It does not prove semantic-key idempotency across
different message IDs, aggregate ordering, replicas, OTLP export, quarantine
replay, wire ACK loss, killed-transaction recovery, broker or PostgreSQL restart,
HA or load capacity. Those remain in the
[M02 completion plan](plans/m02-completion.md) or their owning milestones.
No model-provider calls, local model runtime or live trading are involved.
