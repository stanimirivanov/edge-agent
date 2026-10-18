# Multi-process durable acceptance

## TL;DR

- `cargo xtask m02-qualification` provisions dependencies and runs independent
  producer, relay and consumer processes against isolated PostgreSQL schemas
  and owned authenticated NATS brokers, retaining scenario evidence.
- For the focused `event-spine-processes` check only, set
  `EDGEAGENT_POSTGRES_URL` to an isolated administrative test database.
  Docker provisions the broker automatically; alternatively
  `EDGEAGENT_NATS_SERVER` names the absolute pinned 2.14.7 native executable.
- Acceptance/outbox and inbox/probe/fact intent each share one commit. External
  publication occurs only through a separate production relay invocation.
- Semantic receipts and aggregate-local revision guards tolerate distinct
  message IDs, out-of-order work and competing replicas without global FIFO.
- The [qualification command](m02-qualification.md) additionally proves specific
  worker kills, same-store broker restart and directional wire ACK loss. This
  does not establish model durability or production service readiness.

## Composition and ownership

The `conformance-worker` binary belongs to the tooling-only
`edgeagent-event-spine-conformance` package. It is not an image/deployable and
does not alter product shells. [ADR-0026](../decisions/0026-compose-durable-acceptance-in-independent-processes.md)
records ownership, lifecycle and compatibility decisions.

1. A fixture administrator creates fresh producer/consumer schemas, applies
   outbox migrations to both, inbox migrations to the consumer, and only the
   owning synthetic acceptance or probe/semantic-receipt tables to each schema.
2. It creates seven restricted logins: producer, producer relay, consumer,
   consumer relay, each owning schema's operator and the consumer dispatcher.
   None owns a schema or receives migration authority. A relay
   can select/update only its outbox. Peer-schema writes and runtime DDL fail.
3. The authenticated provisioner creates registry-derived topology before any
   runtime child starts. Runtime broker permissions come from the same plan.
4. A producer child connects, reports `ready`, waits for one parent action byte,
   writes acceptance plus command outbox, commits and exits `accepted`.
5. A fresh producer-relay child claims/publishes/records one intent and exits
   `published`; it holds no database transaction while publishing.
6. A consumer child receives one command, validates it, atomically writes inbox,
   semantic receipt, revision/value and fact intent, commits, confirms ACK and
   exits `applied`. A committed transport duplicate exits `duplicate` without
   invoking the callback. A semantic duplicate commits its distinct inbox but
   writes no second receipt, transition or fact.
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
inbox, state, semantic receipt and fact enqueue, proving all four roll back before retry is
confirmed. A fresh consumer receives the retained command and commits normally.
These synthetic controls remain outside production adapter APIs.

`semantics::competing_semantic_commands_commit_one_transition_and_retain_conflicts`
holds two independent deliveries before releasing competing replicas. Distinct
transport IDs for the same probe/revision/value produce two inbox receipts but
one state transition, semantic receipt and causal fact. A changed value enters
`semantic_conflict` quarantine with the exact original bytes; accepted state,
receipt and fact remain unchanged.

`semantics::missing_predecessors_recover_in_order_or_exhaust_into_quarantine`
delivers revision one before revision zero. It proves the gap rolls back all
writes, then holds its redelivery while another process commits the predecessor.
Releasing the successor advances state to revision two. An old semantic duplicate
still compares accepted history, not current value. A separate unresolved gap
requests four retries, then retains `missing_predecessor` quarantine on delivery
five without a partial inbox, provisional state, receipt or fact.

`replicas::competing_replicas_respect_shared_inflight_high_water` provisions a
durable bound of two outstanding ACKs with four waiting pulls allowed. Two child
processes hold deliveries and a third has a confirmed waiting pull, with one
message still pending. No transaction/state exists yet. Only releasing a held
delivery through committed processing/ACK admits the third; all three finish
with one receipt/state/fact each. Broker state, not a timing threshold, proves
the admission bound. Each worker owns one connection/delivery and does not prefetch.

`replicas::locked_partition_and_connection_loss_preserve_other_progress_and_recovery`
observes a worker blocked by an administrator-held aggregate row lock. Another
partition commits before the lock is released. The fixture then terminates only
that observed worker connection mid-transaction; confirmed retry and durable
inspection show no partial inbox/receipt/state/fact. A fresh process consumes the
retained command and advances the aggregate once.

## Semantic and ordering policy

[ADR-0027](../decisions/0027-enforce-service-owned-semantic-command-ordering.md)
defines the conformance-specific policy. Semantic identity is
`(probe_id, expected_revision)`; content compares validated `value`, not message
ID, envelope metadata or unknown optional fields. Accepted receipts are immutable
and remain available after later revisions. Under the probe row lock, an identical
receipt is a no-op; changed content conflicts. With no receipt, only the exact
current predecessor permits a guarded update and receipt/fact insertion. A future
revision defers; an unexplained stale revision fails permanently.

A provisional revision-zero row serializes contenders for a previously absent
aggregate and rolls back with an unfulfilled predecessor. Locks are per aggregate,
not global, and have the existing two-second timeout. This proves safe concurrent
admission, not FIFO, starvation freedom, throughput, or unbounded storage. The
portable inbox does not acquire business-key policy. `applied` means that a first
transport identity committed; inspect receipt/state/fact counts to distinguish a
new transition from an acknowledged semantic duplicate.

Facts derive stable identity from probe/revision and causation/correlation from
the winning command. Facts also preserve that command's validated W3C carrier;
the proof exporter's stage spans link it without rewriting accepted bytes.
Producer acceptance is still transport-identity based;
consumer semantics decide whether its independently published request changes state.

## Lifecycle and failure contract

**[Prerequisite from M10]** Each child handles one operation only: two runtime
threads, one connection, one delivery or claim. Readiness follows actual
connection, not elapsed time. The parent starts work with one byte (`c` commit,
`r` fixture rollback), closes stdin, then checks a bounded result and exit.
For an explicit producer command, bounded structured envelope bytes follow the
action and end at EOF; absent bytes select the original default fixture. The
consumer-only `h` action emits `delivered` after receive and waits for a `c`
release byte before opening any transaction. This fixture barrier exists only
inside tooling, not production adapters. A unique worker application name lets
the administrator identify owned lock waits without exposing credentials.
Children have a 30-second total deadline; the proof has a tighter 15-second child
budget and 90-second database-scenario budget. SQL statements are limited to 5 seconds,
locks to 2 seconds, broker operations/receive to 5 seconds. Runtime shutdown is
bounded even if a parent abandons stdin. No arbitrary sleep coordinates work.

The parent kills/reaps owned children on ordinary errors/deadlines; kill-on-drop
is a cancellation backstop. Owned database and broker fixtures attempt bounded
cleanup on errors, panics and deadlines. Disposable CI teardown handles process
abort. Unknown or cancelled outcomes are not confirmed rollback/success; retained
identity and outbox/inbox evidence determine safe re-entry. Never rerun with
new message IDs to conceal an uncertain result.

Worker errors intentionally omit source chains and payloads. A failed child
returns unresolved failure, not a fabricated durable success. A classified
settlement failure reports `confirmation_unknown`; it never reports `applied`.
Oversized raw intake reports `protocol_halt` without ACK/quarantine. Tests distinguish
explicit rollback, committed success, duplicate and idle outcomes. Neither child
exit nor broker health alone constitutes transaction evidence.

## Verification and metadata

Default `cargo xtask verify` compiles the worker and discovers the ignored T3
suite; it does not execute service-backed proofs. `cargo xtask m02-qualification`
provisions isolated dependencies, runs protected suites, retains reports and
cleans up. Missing prerequisites fail, never skip. The focused check below
still accepts an explicitly managed isolated PostgreSQL environment.

```text
cargo xtask verify --check event-spine-processes
```

All service-owned statements use checked `sqlx::query!` and committed
offline metadata. The [metadata action](sqlx-metadata.md) also applies
`crates/event-spine-conformance/fixtures/process-schema.sql` in its authorized
metadata database before preparing all targets. Test-only schema/grant/inspection
queries remain dynamic; no untrusted message fields enter SQL identifiers.

## Limits and follow-ups

This is bounded synthetic probe behavior, not product research/order policy.
Use fresh disposable schemas: old retained fixtures constrain revision to one
and lack semantic receipt history. Neither mixed-version workers nor an upgrade
of retained synthetic state is supported. Never infer old receipts from current
state or apply the fixture to a product deployment.

The canonical gate admits at most two scenarios concurrently, independently of
host CPU count. Each scenario retains its internal competing workers and its
original deadlines; this bounds fixture pressure, not the behavior under test.

The nineteen-case protected suite includes four
[authorized replay proofs](quarantine-replay.md): authenticated/atomic authorization,
inbound result-loss recovery and bounded fencing, original-route fan-out, and
outbound audited release. Replay authorization needs no broker connection;
dispatch uses a separate login and exact-subject broker scope. Its fixture-only
`p` barrier reports `persisted` after a real broker receipt, then waits for `c`
outside any transaction so a parent can kill or fence that holder.

Two [exported telemetry proofs](event-spine-telemetry.md) add an owned OTLP/HTTP
receiver, non-default W3C links, exact metric dimensions/counts, raw-wire privacy
sentinels and real child PID evidence. Rejected or stalled exports must preserve
commits, duplicate suppression and confirmed settlement. Export is opt-in in
child-only environment; normal process cases remain unconfigured. Bounded SDK
threads and HTTP transport are installed before Tokio and shut down after it;
their diagnostic result never replaces the worker's operation result.

Seven [failure qualification cases](m02-qualification.md) add producer, relay
and consumer process kills, same-store broker restart, directional wire ACK
loss, duplicates after broker deduplication expiry and invalid-input bounds.
Fixture-only `b`/`e` producer actions hold before commit/after enqueue, `u`
holds a real relay receipt before completion, and `a` holds postcommit
settlement. These closed actions accept no arbitrary payload. Consumer `v`
reports actual delivery attempt before a `c` release. All stay out of adapter APIs.

The proof does not establish production telemetry operation, arbitrary crash
or network faults, PostgreSQL restart, power-loss, HA or load capacity.
Connection interruption is not a database restart or an unknown-commit proof.
Those remain in the
[M02 completion plan](plans/m02-completion.md) or their owning milestones.
No model-provider calls, local model runtime or live trading are involved.
