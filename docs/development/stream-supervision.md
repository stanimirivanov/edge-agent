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

PR20B implements protected incidents/restore barriers and exact original
preparation recovery through the existing stream publication ledger, including
uncertain commit results. PR20C must compose owner-pinned IEX semantics, deterministic
identities, authority, bounded tasks, publication and domain readiness. Neither
task may reset budgets, substitute sources or infer physical quiescence from a
timeout, cancellation or dead process. See [ADR-0069](../decisions/0069-bound-offline-stream-supervision-with-unresolved-heads.md).

## Verification and rollout

PR20B adds [ADR-0070](../decisions/0070-fence-stream-restores-with-durable-incident-barriers.md)
and migration `0009_stream_recovery.sql`, after 0008. New and existing sessions
start blocked at revision zero. Grant the independently provisioned worker LOGIN
only `EXECUTE ON FUNCTION edgeagent_stream_recovery_control(TEXT,BIGINT,TEXT,SMALLINT)`;
never grant recovery-table DML, including through role membership. Existing owner,
account, retention and current-catalog checks remain required. Daily behavior is unchanged.

Every worker restart must call `begin_restore` before reconstructing its transient
scheduler. This commits a higher blocked revision. If its COMMIT result is unknown,
inspect durable state and begin another restore; do not assume rollback or reuse
an old approved snapshot. `StreamRestore` is a local revision witness, not permission.
Only `approve_stream_restore` on the independent owner's LOGIN can approve that
exact revision, after the owner independently verifies old workers are fenced and
restore/reference/consumer inputs are current. Approval rejects pending publication
intents. `record_stream_incident` irreversibly blocks that session; inspect state
after an uncertain result. Reconciliation needs a separately admitted session.

Call `recover_pending_stream_segment` for each explicitly selected instrument
(selection remains capped at 32). It discovers the sole pending lane preparation,
uses its original bytes/IDs/event and rechecks the restoring fence before/after I/O.
Unknown PUT results remain inventoried. A committed-but-unconfirmed publication is
not pending: recover its retained exact receipt with `recover_segment` and
`recover_publication` when its ID is known; do not issue another PUT or invent an ID.
An empty lane proves no unfinished preparation, not absence of past captures.
Expired/stopped preparations require the existing authorized retirement path;
they cannot silently reopen the same session. Neither recovery nor owner approval
refunds usage, releases an unresolved provider generation or certifies market readiness.

`check_stream_restored` must be checked afresh at use time. It denies stale,
unapproved or incident-bearing revisions, stopped admissions, pending intents and
current authority loss. Existing capture APIs still only capture; they do not
enforce this new readiness barrier automatically. PR20C must compose checks on
both sides of bounded work, IEX semantic/identity acceptance and domain readiness.
Do not expose a streaming runtime until that composition is qualified.

Run `cargo xtask verify --check streaming-segment-contracts` for pure recovery
proofs, and `cargo xtask verify --check streaming-publication-postgres` on the
isolated local platform for role, restart, uncertainty, rollback and exact receipt
proofs. `cargo xtask verify sqlx-metadata` checks the typed function/cache against
the actual migrated schema in CI. These are distinct evidence boundaries.

`cargo xtask verify --check stream-supervision-contracts` runs six deterministic,
credential-free proofs with an independently protected discovery census.
`cargo xtask verify` includes that gate on Ubuntu in the existing
Repository quality workflow. The proofs cover saturated 32-source fairness,
FIFO/duplicate preservation, slow/failed-source isolation, both capacity ceilings,
foreign/dropped dispatches, cancellation, binding and redaction. They use no
Docker, network, provider credits or mutable wall clock.

This additive library changes no wire format, persistence, runtime command,
dependencies or deployment. Rollback removes library use; never treat a library
rollback as permission to erase durable uncertainty. Actual streaming activation
and the outstanding daily live qualification remain explicitly not run.
