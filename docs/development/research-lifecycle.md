# Explicit owner artifact lifecycle

## TL;DR

- Research appends immutable lifecycle revisions and outbox events; artifact bytes never change.
- An explicit trusted local operator command supplies effective time; no scheduler runs.
- Projector gaps remain visible until predecessors and the original delivery are retried.
- Gateway metadata includes the latest lifecycle fact; closed outcomes return 410.

Configured opt-in checks are documented in [Research invalidation](research-invalidation.md).

## Owner command and policy

Prepare every Research migration, including
[0002_research_lifecycle.sql](../../crates/research-postgres/migrations/0002_research_lifecycle.sql),
with existing inbox/outbox migrations in the same independent private schema.
The additive migration creates an immutable lifecycle table and indexed exact
outcome-digest lookup. No existing completion row changes. Do not prune completion,
lifecycle or outbox integrity records independently.

`edgeagent-research offline-lifecycle` uses the existing loopback database,
`EDGEAGENT_RESEARCH_SCHEMA` and `EDGEAGENT_RESEARCH_SCOPE` configuration. Closed stdin
contains one canonical `ResearchRecord::Lifecycle` (at most 131072 bytes). Successful
stdout is the exact original owner event envelope. Retry identical commands after
lost responses; changed content at an existing revision conflicts. Errors produce
only the existing safe CLI failure diagnostic. This command requires a prepared
owner schema but no broker, evidence loader, model, provider or external authority.
The caller must already be the trusted local owner operator; the body cannot grant
that authority and its scope must match configuration. No Gateway mutation API or
public authentication profile is added.

| Current state | Allowed next state |
|---|---|
| published | invalidated, expired, withdrawn, corrected |
| invalidated or expired | withdrawn or corrected |
| withdrawn or corrected | none |

Owner lifecycle revisions start at one, are consecutive, and have nondecreasing
explicit effective times at or after the original decision. Expired requires the
original artifact expiry cutoff to have been reached at that effective time.
Invalidation/withdrawal/correction can happen before expiry. A repeated state is
not a new transition; only the exact prior record is an idempotent duplicate.
The graph permits at most two lifecycle changes and never reactivates an artifact.
These are explicit historical owner facts; the command does not compare a host
clock to effective time or automatically expire anything.

A correction names a separately committed artifact digest. It must be intact and
in the same scope, share decision time, lookback and universe, have a distinct
request ID, remain published with no lifecycle history, and be unexpired at the
correction's effective time. Implementation/configuration may differ to repair a
calculation. The target is not calculated or published by this command. Scope-wide
transactional advisory serialization fences target-state races and correction
cycles: linking back to an already corrected/closed artifact is refused. No graph
walk, peer-table read, copied replacement or revival occurs. The public correction
link is a digest, not an arbitrary fetch URL or automatic redirect.

## Durability and publication

The [application validator](../../crates/research-application/src/lifecycle.rs)
binds bounded history to original artifact bytes and maps into the independent
[pure state policy](../../crates/research-domain/src/lifecycle.rs). The owner store
validates retained completion/publication/outbox integrity and every retained
lifecycle event before accepting new work. A scoped advisory lock serializes
lifecycle commands; hash collisions only add serialization. READ COMMITTED,
five-second lock and fifteen-second statement bounds apply, with a 25-second CLI
deadline. One transaction appends the lifecycle record and exact outbox event.
No immutable original is updated and no successful event exists after rollback.

The additive Research-owned `com.edgeagent.research.lifecycle-changed.v1` event
uses the existing record schema and scoped research-request partition. Record
digest is event ID/idempotency key; original artifact digest is correlation ID.
Existing finite `offline-relay` publishes it through the normal outbox registry.
Source/route/schema/binding validation is not cryptographic owner authentication;
broker and local operator provisioning remain trusted outer obligations.

## Projection, queries and gaps

The pure Research reducer retains original outcome bytes plus at most two exact
lifecycle records. Existing empty-history EARP1 checkpoints remain byte-compatible;
nonempty histories use EARP2 and reconstruct status from validated records. Local
projection revision is one plus lifecycle revision; it is not a global cursor.
The latest successfully applied delivery key advances atomically with each new
fact's checkpoint and inbox receipt; duplicates keep it unchanged.

Before the base is known, lifecycle input returns Gap without creating a fake
request anchor or successful receipt. The broker retains the unacknowledged input;
manual callers must retain/retry their own stdin. For an existing request, retain
one bounded missing-revision envelope while accepting its missing predecessors.
Keep metadata gapped and block outcome reads until the original retained input is
successfully retried and the gap is cleared in the same commit. Conflicts stay
blocked; no revision is skipped or silently repaired. Broker delivery ceilings may
require operator backfill/replay; there is no automatic unbounded retry scheduler.

The projector durable must now filter all three PUBLISHED, ABSTAINED and CHANGED
subjects with the existing explicit-ACK/delivery bounds. Stop intake, upgrade the
filter set while preserving durable progress, then activate the new worker. Old
two-subject configuration is refused. Each retained lifecycle record must have its
own intact projector inbox proof before queries succeed.

Metadata adds optional `lifecycle` with the latest full owner fact, including a
correction digest, and supports invalidated/expired/withdrawn/corrected status.
Gateway validates its scope/ID, contract, revision and state relation. Metadata
with an unresolved gap can also carry the last applied lifecycle fact. Original
pending/artifact/abstained response shapes remain unchanged. On closed views,
`/outcome` returns 410 with that status; original immutable bytes remain retained
for audit/rebuild. Metadata is an observation and cannot certify current owner
state, evidence rights or entitlement. Gateway applies its existing local scope,
HTTP bounds and no-store policy, and never follows correction links automatically.

## Verification and rollback

Run `cargo xtask verify --check research-lifecycle --check research-projection-contracts --check gateway-query-contracts --python python3`
for pure time/state/replay and Gateway metadata/closed-response proofs. With explicit
isolated loopback PostgreSQL, run
`cargo xtask verify --check research-persistence --check projector-postgres --check research-worker-postgres --python python3`.
These cover owner concurrency/idempotency/cycle refusal, statement/deferred-commit
rollback, actual owner CLI restart, missing-predecessor backfill, atomic gap clearing,
closed HTTP responses and retained-proof corruption. Add loopback NATS and run
`cargo xtask verify --check projector-broker --python python3` for real lifecycle ACK
and projection. Default verification protects exact test discovery. Canonical SQLx
assembly automatically discovers the additive migration; missing CLI prerequisites
remain explicit, even when live metadata generation/offline compilation pass.

Stop lifecycle intake and relay before rolling back dependent consumers. Old code
cannot read EARP2 or understand the new route/status; preserve owner histories and
all pending events, and rebuild only disposable projections with compatible code.
Do not drop authoritative lifecycle history to make old binaries appear current.
Autonomous detection, schedules, public mutation APIs and complete four-process
qualification remain outside this slice.
