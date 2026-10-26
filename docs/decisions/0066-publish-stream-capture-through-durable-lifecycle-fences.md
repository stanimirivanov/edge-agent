# ADR-0066: Publish stream capture through durable lifecycle fences

- Status: Accepted
- Date: 2026-10-26
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Inventory exact preparations before object PUT; retry the original bytes and IDs.
- Atomically advance capture heads, private receipts and opaque outbox notifications.
- Cancellation stops capture, not previously published licensed reads.
- Withdrawal denies reads immediately; deletion needs independent writer quiescence.

## Context

Immutable stream objects alone cannot establish durable publication or safe cleanup.
Frame accounting survives reconnects, but earlier generations did not retain their
starting frame. Retained rights and capture execution have different lifetimes.

## Decision

Serialize publication by account, protected admission, session, retention and lane.
Record the first ordinal of each new connection generation; unknown legacy starts
fail closed until independently confirmed source quiescence and a new reservation.
Do not refund usage or release unresolved slots during migration or cleanup.

Persist opaque publication intent, exact frozen bytes/event metadata and deletion
inventory before external storage I/O. Conditional PUT and verified read-back remain
outside SQL. Commit the original private receipt, lane head and transactional outbox
together. Recover committed outcomes without another PUT or resetting capture.
Different intents cannot fork a lane or overlap its published local capture range.

Notifications contain only opaque session/publication IDs and use the existing audit
retention class, longer than finite session plus licensed capture lifetime. Private receipts expose
explicit first/retired-history, reconnect/epoch and skipped-frame boundaries. These
are capture checkpoints, never continuity, quality, freshness or evidence readiness.

Capture cancellation fences new publication but preserves existing reads under
current exact rights and original exclusive expiry. Withdrawal denies reads and
publication immediately. Check durable denial before and after storage I/O; do not
return data if withdrawal wins before the final authorization check.

Retirement erases private preparation, receipt, head digest and retained outbox bytes while
keeping the exact deletion descriptor. Published unexpired segments cannot be retired
merely because capture stopped. An independent owner must irreversibly stop writers
and confirm storage-write quiescence; a separate per-object cleanup authority must
confirm the exact descriptor. Only confirmed deletion removes inventory. Retain opaque
denial/counter/capacity tombstones, erase licensed admission witnesses after cleanup.

## Alternatives considered

- PUT before inventory: rejected; uncertain writes would become untracked orphans.
- Hashes on the bus: rejected; broker retention can outlive licensed fingerprint use.
- Stop equals revoke: rejected; cancellation must not destroy retained access rights.
- Timeout means quiescence: rejected; late PUTs can recreate deleted objects.
- Frame adjacency means source continuity: rejected; filtered/control frames differ.

## Consequences

Recovery is exact and bounded, at the cost of private preparations retained until
publication or authorized retirement. Active lanes retain hash-free capture positions
so retirement cannot spend old frames again; uncertain unpublished work holds its lane
until capture stops. Erase lane metadata when stopped with every segment retired.
Session/account ceilings bound work; one pending
preparation per lane prevents parallel speculative objects. Store IAM, independent
operator assertions and authenticated reference catalogs remain trusted boundaries.

## Compatibility and migration

Apply migration 0008 after 0007 before new workers. Provision retention owner controls
separately from worker DML; grant only protected read functions and SELECT on controls.
The additive event has a new schema/route; daily formats remain unchanged. Stop new
workers before rollback; retain additive tables and uncertain objects/connection slots.
Never run old workers against new capture publication without generation fencing.

## Security and operations

No vendors, WSS runtime, credentials or model calls are activated. Provider connection
quiescence and storage-writer quiescence are independent facts. Cleanup never frees a
provider slot. Operator cleanup uses explicit scope and bounded batches (1..16), no
bucket-wide listing or sweeping. A notification is not a bearer read capability.

## Validation

`cargo xtask verify --check streaming-segment-contracts` covers synthetic encoding,
retained rights and isolated namespace deletion. `cargo xtask verify --check
streaming-publication-postgres` covers real PostgreSQL roles, exact recovery,
post-enqueue rollback, stale generations, checkpoints and lifecycle fences. The default
`cargo xtask verify` protects discovery, architecture, contracts, linting and docs.
