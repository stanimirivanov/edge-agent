# ADR-0070: Fence stream restores with durable incident barriers

- Status: Accepted
- Date: 2026-10-27
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Persist opaque supervision incidents and monotonically fenced restore revisions.
- Default every session to blocked; an independent owner approves an exact restore.
- Reuse inventoried publication bytes and receipts; never refreeze after uncertainty.
- Keep capture, restored state and domain evidence readiness separate.

## Context

PR20A's queues and health are transient. Rebuilding them after a crash cannot
clear an incident, establish current reference/consumer state or prove publication
completion. PR17B already owns exact preparations, receipts and atomic outbox
publication; another journal or regenerated events would split that ownership.
The maintainer approved PR20B as the offline durable recovery slice.

## Decision

Add protected session-local restore control. Missing control is installed blocked
at revision zero, including existing admissions. `begin_restore` commits a higher
blocked revision before returning a local witness. Older witnesses cannot recover
or pass restore checks. Unknown commit results remain blocked; inspect retained
state and begin a new restore rather than assuming a failed mutation rolled back.

Only the independent admission owner's authenticated LOGIN can approve the exact
revision. Approval requires no sticky incident or unpublished/unretired preparation,
current capture authority and an unstopped admission. The owner independently
asserts prior workers are fenced and restore/reference/consumer inputs are current.
Workers cannot edit control rows, approve a revision or clear an incident through
the worker port. Incidents permanently block this finite session; reconciliation
requires a separately admitted session, not a counter reset or automatic failover.

Recover one selected instrument lane per call through `pending_segment` and the
existing publication coordinator. Reuse exact preparation, IDs, digest, predecessor
and event metadata. Recheck the restore fence before and after external work.
Already committed outcomes use retained receipts without another PUT. No pending
intent means only no unfinished preparation, not that no prior commit occurred.
Recovery never approves its own result.

## Alternatives considered

- In-memory incident/reset flags: process loss erases the safety barrier.
- Worker-approved restore: makes the consumer of authority its own approver.
- Regenerated capture: changes identities, timestamps and lineage after a crash.
- Publication as readiness: ignores correction, rights and freshness barriers.
- New journal/bus: duplicates the established exact publication boundary.

## Consequences

Every new worker must explicitly begin restore before reconstructing a scheduler.
Object I/O remains outside SQL; post-effect denial can coexist with a committed
capture, so recover receipts rather than compensate or retry with new IDs.
Low-level immutable captures remain capture records, not qualified evidence.
PR20C must compose fresh barrier checks, owner-authenticated semantics, bounded
workers and domain readiness before any streaming runtime use.

## Compatibility and migration

Apply migration 0009 after 0008. It is additive and starts existing sessions blocked.
Grant workers only EXECUTE on the recovery function, never control-table DML.
The checked SQLx function signature/cache participates in CI schema regeneration.
No wire format or daily behavior changes. Rollback stops recovery consumers while
retaining incidents, revisions and original usage/account uncertainty.

## Security and operations

Early-bound SQL and a fixed `pg_catalog` search path prevent temporary shadow
tables from granting approval. Mutations follow account/admission/session/control
lock order. Approval/pending inspection serialize with publication session locks.
Worker LOGINs with control write memberships are denied. Opaque incident codes
contain no licensed payload; existing scoped retirement still governs captures.
No timeout, cancellation, process loss or recovery success releases connection
slots, renews rights or proves external/storage quiescence. No vendor activation ships.

## Validation

Protected pure proofs run in `streaming-segment-contracts`; isolated PostgreSQL
role, incident, rollback and exact receipt proofs run in `streaming-publication-postgres`.
CI also regenerates SQLx metadata. See [stream supervision](../development/stream-supervision.md).
