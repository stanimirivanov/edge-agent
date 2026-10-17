# ADR-0027: Enforce service-owned semantic command ordering

- Status: Accepted
- Date: 2026-10-17
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Keep semantic command identity and predecessor policy in the owning synthetic
  service, separate from portable inbox transport deduplication.
- Serialize only one probe aggregate at a time; commit its immutable semantic
  receipt, revision, inbox and outbound fact intent together.
- Defer missing predecessors with bounded retries; retain conflicts and exhausted
  gaps in quarantine without changing accepted evidence.
- Prove shared replica admission and connection-loss recovery with real child
  processes and observable barriers, not throughput thresholds.

## Context

The process composition in ADR-0026 proves immutable transport identity and one
initial transition. Different transport IDs can still express the same command,
and a partition key alone establishes neither FIFO nor safe replica concurrency.
M02 slice 4 must demonstrate business-level idempotency, ordered recovery and
resource bounds without inventing product research or order semantics.

## Decision

For the conformance-only `ApplyProbe` contract, semantic identity is the pair
`(probe_id, expected_revision)`. Content equality compares its validated `value`.
Optional unknown wire fields do not change that v1 meaning. Transport IDs,
correlation, causation and trace metadata do not define semantic equality. The
portable inbox still compares immutable transport identity/content independently.

Within the existing inbound transaction, create a provisional revision-zero
probe if absent, then acquire its row lock. Read its immutable command receipt
under that lock. An identical accepted command succeeds without changing the
aggregate or adding a fact, even after later revisions; a changed value produces
`semantic_conflict`. Without a receipt, the exact predecessor permits a guarded
revision update, receipt insert and fact enqueue. A future predecessor produces
transient `missing_predecessor`; an unexplained stale revision fails permanently.
Rollback removes provisional rows and inbox evidence as well as other writes.

Every transition has a deterministic fact identity derived from probe/revision;
the winning command supplies causation and correlation. A separate relay publishes
only after commit. Different semantic duplicates still retain distinct inbox
receipts, so an `Applied` coordinator result means that first transport identity
committed, not necessarily that it caused a new aggregate transition.

Replicas share a durable consumer and its configured outstanding-ACK bound. Each
finite worker has one delivery, one connection and no background prefetch. A
tooling-only stdin barrier after receive exposes admission before any SQL
transaction. Administrator-held aggregate locks and observed PostgreSQL lock
waits expose contention and connection termination. No adapter gains test hooks.

## Alternatives considered

- Deduplicate semantic keys in the portable inbox: would import service policy
  into a transport-identity port and conflate distinct immutable envelopes.
- Trust broker FIFO or a partition extension: cannot serialize competing replicas
  or recover an explicitly missing predecessor.
- Globally lock processing: prevents independent aggregates from progressing.
- Keep only latest state: cannot compare an old command after later revisions.
- Overwrite on key conflict: destroys immutable evidence and hides changed intent.
- Use sleeps or throughput cutoffs: cannot prove admission or transaction order
  deterministically across CI machines.

## Consequences

The synthetic schema gains append-only semantic receipts and revisioned state.
Runtime consumers gain UPDATE on their own probe table, not on accepted receipts;
they still have no peer-schema, migration or replay-audit authority. Checked SQLx
macros describe every worker query, with reproducible offline metadata.

Aggregate-local locking may defer a hot partition at the existing lock timeout;
it does not claim starvation freedom, strict FIFO, or unlimited aggregate capacity.
The bounded handler policy quarantines an unresolved predecessor at attempt five.
Infrastructure unavailability remains retryable, not a fabricated domain failure
or success; this work does not impose a terminal transport outage deadline.

## Compatibility and migration

Published v1 envelopes, schema baselines and portable APIs do not change. The
conformance schema is disposable tooling, never a product deployment migration.
Create fresh proof/metadata databases: old retained synthetic schemas constrained
revision to one and lack receipt history. Do not mix old and new workers over
retained proof schemas or infer historical receipts from current state. Rollback
uses a fresh old-version fixture, not destructive changes to accepted evidence.
ADR-0026 remains the historical composition decision; this adds its planned
semantic-ordering slice rather than rewriting it.

## Security and operations

**[Prerequisite from M10]** Admission, finite worker lifecycle and deadlines remain
limited proof requirements. Tests configure a two-delivery shared high-water
mark and explicitly show a third waiting pull; releasing an ACK admits it.
Workers receive bounded producer envelope bytes on stdin, validate them before
acceptance, clear ambient credentials and redact failures. Interrupted work
recovers using retained identity after confirmed retry; cancellation or unknown
outcomes never imply successful commit or permission to compensate.

Connection termination selects exactly the owned worker application name and
restricted role while its transaction is observed waiting on a fixture lock.
It never restarts PostgreSQL or interrupts another project's sessions. Existing
bounded fixture teardown remains responsible for owned resources. No model,
live-order, cloud or production runtime capability is introduced.

## Validation

The protected `event-spine-processes` inventory includes competing semantic
commands/conflicts, out-of-order recovery/retry exhaustion, shared admission,
and unrelated progress during an aggregate lock plus connection-loss recovery.
Assertions inspect committed inbox, state, receipts, exact quarantine bytes and
outbox facts. Default tests prove policy and enforce missing/renamed/duplicate/
extra census mutations; only T3 execution proves database/broker/process behavior.
See the [process guide](../development/process-acceptance.md) for invocation and
limits. Full crash, wire-ACK-loss, replay and exported telemetry remain later M02
slices; resource bounds are not load, HA or cloud qualification.
