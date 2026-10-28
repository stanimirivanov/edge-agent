# Bounded offline streaming runtime

## TL;DR

- PR20C2C composes the existing IEX acceptance, supervisor, paired publication and domain readiness.
- Use explicit finite synthetic scripts and current owner-approved inputs; no vendor connection or CLI mode is added.
- Start a new durable restore revision before rebuilding state. A separate owner must approve it before execution.
- Run `cargo xtask verify --check offline-stream-runtime` in the PostgreSQL-equipped CI profile.

## Entry point and authority

`services/market-data/src/offline_streaming/` exposes the typed
`OfflineStreamRuntime` service composition API. The executable examples are the
synthetic integration proofs in `services/market-data/tests/offline_stream_runtime.rs`.
This API accepts normalized synthetic frames, not WSS endpoints or credentials.
Existing transport/protocol libraries remain independently qualified; this slice
does not connect them to a live provider or add an operational daemon selector.

Supply sealed sessions, full expected `StreamOwnerInputs`, selection witnesses,
current authority/mapping/calendar snapshots, explicit interpretation profiles,
feed/epoch bindings, continuity policies, clocks, cancellation and storage limits.
Every source must match its approved feed/provider/contract/epoch routing scope.
Snapshot IEX coverage cannot satisfy contiguous evidence requirements.
Trusted reference snapshots are finite caller-owned inputs; external corrections
require owner withdrawal or fencing, not an automatic reference refresh.

The independent owner LOGIN installs inputs and approves restore witnesses. The
restricted worker LOGIN never installs or approves its own inputs. `begin` starts
blocked revisions for all sources, authenticates actual protected input records,
and reconstructs complete bounded history. Obtain the exact witnesses through
`restores()`, approve them using the existing owner ledger, then call `run` once.
An unapproved readonly gate consumes no script or account budget.

## Recovery and publication order

1. Inventory metadata before loading preparation bodies. Include tombstones and
   pending entries; inspect aggregate row/byte bounds before object reads.
2. Recover only original pending canonical/native pairs under a `Restoring` fence.
   Reuse original IDs, bytes, receipt times and outbox events. This phase cannot
   reserve connections, charge arrivals or stage a replacement intent.
3. Read exact receipt-bound pairs and reaccept original native records under the
   current complete profile/references. Rebuild continuity and predecessor chains.
   Erased, expired, legacy canonical-only, incomplete or over-limit history fails
   closed; reconciliation/new admission is required rather than partial deduplication.
4. Independently approve the exact restore revision. Revalidate retained receipts
   and rebuild the activity baseline without synthesizing an activity pulse.
5. Reserve explicit synthetic connections; charge each complete frame before
   acceptance. Round-robin intake skips unresolved sources. Group a frame's selected
   records for atomic queue confirmation and partition publication by instrument.
6. Reserve the shared retained-history budget and stage every frame proposal before
   any PUT. Write/verify both immutable objects outside SQL; commit the existing
   receipt/checkpoint/outbox transaction under the exact running revision.
7. Confirm the unresolved frame head only after all its publications commit.

`PostgresStreamingLedger::fenced` checks revision/phase within existing locked SQL
transactions at accounting and publication boundaries. All actors participating in
this runtime must use it. Older unfenced capture APIs remain compatible and do not
grant readiness; do not mix them with runtime actors. The independent owner's
assertion that previous workers are fenced remains necessary. This API does not
restrict arbitrary direct DML by a trusted database principal.

## Bounds, failure and readiness

All limits are explicit. Portable ceilings are 32 sources, concurrency no greater
than source count, 4096 pending records/frames/retained records, 1024 records per
synthetic frame, 128 retained history entries and 16 MiB preparation history.
Per-object limits remain 4 MiB/4096 records. A shared budget includes restored rows,
new staged proposals and uncertain reservations, without refunds. These ceilings
bound retained preparation history, not total process heap.

Begin/run work has an explicit deadline of at most 60 seconds. Individual I/O has
at most 15 seconds, no longer than the work deadline. A failed run receives one
additional global I/O budget to establish durable fences. No task is detached.
Explicit activity pulses never rewrite source event or receipt times. Synthetic
byte counts are test inputs, not a measurement of live provider traffic.

Semantic conflicts and overload latch sticky incidents. Operational uncertainty,
cancellation or I/O failure requires a new blocked restore. Unconfirmed durable
mutation returns `Unconfirmed`; local state cannot claim that a barrier persisted.
Dropping a run consumes the local attempt, preserves unresolved ownership and
requires a new runtime/restore. There is no asynchronous Drop, automatic retry,
reconnect, replacement ID, quota refund or connection release. Only independent
owner-confirmed quiescence permits the existing connection-release procedure.

After successful finite execution, `ready` rechecks source health, current actual
inputs/restore approval, original receipt expiry/retirement and exact manifest
bindings, then applies the existing domain `ReadinessPolicy`. The cache stores
original member descriptors and native values; it is not authority. Empty healthy
queues, restore approval, old history without activity and failed work cannot grant
readiness. The result is borrowed point-in-time `ReadyEvidence`, not a reusable
authorization token. No history/read receipt is refreshed to make evidence young.

## Verification and rollback

The owned `offline-stream-runtime` proof contract runs four ignored PostgreSQL
tests once through `local-platform`: approval and grouped publication/readiness;
slow-source isolation; state-loss recovery and stale-worker fencing; sticky
failures and revocation. An independent literal census rejects missing tests.
The existing `stream-supervision-contracts` and `iex-canonical-acceptance` gates
cover the pure grouping and interpretation paths. `sqlx-metadata` verifies the new
history query against the migrated database; offline compilation alone does not.

There is no DDL or wire-format migration. Roll back binaries while retaining
preparations, receipts, tombstones, namespace claims and unresolved connections.
Preserve owner barriers during rollback and do not restart unfenced capture actors
against the same admission. CI qualifies the synthetic runtime; live vendor
activation and original daily live qualification remain separately blocked/not run.
