# Offline bounded stream supervision

## TL;DR

- PR20A adds bounded canonical-record queues and round-robin dispatch, not a worker.
- Explicit per-source and aggregate bounds include unresolved heads; no eviction.
- Dropped dispatches stay unresolved; overload/failure/cancellation never retry or refund.
- Healthy scheduling is not evidence readiness, accepted publication or authorization.
- PR20B and PR20C must precede streaming runtime use; vendor activation stays blocked.

## Local scheduling contract

`StreamSupervisor` in `market-data-application` admits 1..=32 distinct `FeedId`
lanes, each pinned to one epoch, provider declaration and positive queue capacity.
The caller also supplies a positive aggregate capacity. Both record ceilings are
at most 4096; these are portable maxima, not operating defaults. Capacity includes
the unresolved FIFO head. Queues allocate lazily and accept only bounded typed
`CanonicalObservation`, never raw provider buffers or arbitrarily sized batches.
The caller must bound its producer buffers and producer-task count separately.

`enqueue` checks the exact lane epoch/provider declaration. This does not
authenticate the provider, conditions, calendar, identity history, rights or
reference provenance. PR20C must supply those decisions before IEX records enter
this queue. Duplicate arrivals are preserved with original IDs and timestamps;
the existing domain continuity tracker owns semantic deduplication and readiness.
No receipt clock is sampled and no local arrival ordinal becomes a source sequence.

One `dispatch_next` call examines at most one rotation. It skips empty, blocked
and unresolved lanes, marks one exact head unresolved and moves the cursor past
that lane. A continuously eligible lane waits at most one rotation, independent
of a hotter source's backlog. This is record/call fairness, not byte-weighted
fairness or a wall-clock latency guarantee. Consumers must not await slow work
before giving other lanes their bounded turns.

The non-cloneable dispatch holds the immutable observation and a private local
owner witness. `confirm` removes only that supervisor's exact FIFO head after an
explicit caller assertion. It does not prove a database, object-store or broker
commit. Passing a foreign handle changes neither queue; the original source
remains unresolved. Dropping a dispatch never removes its head or starts recovery.
There is no automatic retry or mutable queue escape hatch.

## Failures and health

Per-source or aggregate overload rejects and returns the exact incoming record,
latches the affected lane and preserves all retained heads. Explicit source
failure and binding mismatch likewise stop the lane. Other sources may drain,
but aggregate `check_health` fails. The first lane incident remains sticky.
Cancellation is checked before enqueue, dispatch and confirmation and is visible
immediately to health/status queries. It stops all new work without claiming
rollback of effects already performed. Health reports cancellation first, then
incidents, then unresolved work; an empty healthy supervisor is intentionally valid.
It is not a `ReadyEvidence`, freshness check, authority token or quiescence proof.

All payload-owning `Debug` output is redacted. Errors have fixed categories.
The supervisor performs no I/O, logging, spawn, sleeps, cleanup or Drop effects.
It releases no provider account capacity and changes no persisted usage counters.
Memory is transient: process loss loses queues, so this API is not a production
recovery mechanism. Reconstructing it cannot establish permission to resume intake.

PR20B must persist incidents/restore barriers and recover the exact original
preparations through the existing stream publication ledger, including uncertain
commit results. PR20C must compose owner-pinned IEX semantics, deterministic
identities, authority, bounded tasks, publication and domain readiness. Neither
task may reset budgets, substitute sources or infer physical quiescence from a
timeout, cancellation or dead process. See [ADR-0069](../decisions/0069-bound-offline-stream-supervision-with-unresolved-heads.md).

## Verification and rollout

`cargo xtask verify --check stream-supervision-contracts` runs six deterministic,
credential-free proofs with an independently protected discovery census.
`cargo xtask verify` includes that gate on Ubuntu and Windows in the existing
Repository quality workflow. The proofs cover saturated 32-source fairness,
FIFO/duplicate preservation, slow/failed-source isolation, both capacity ceilings,
foreign/dropped dispatches, cancellation, binding and redaction. They use no
Docker, network, provider credits or mutable wall clock.

This additive library changes no wire format, persistence, runtime command,
dependencies or deployment. Rollback removes library use; never treat a library
rollback as permission to erase durable uncertainty. Actual streaming activation
and the outstanding daily live qualification remain explicitly not run.
