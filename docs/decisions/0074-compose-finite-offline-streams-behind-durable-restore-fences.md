# ADR-0074: Compose finite offline streams behind durable restore fences

- Status: Accepted
- Date: 2026-10-28
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Compose finite synthetic stream scripts in the MarketData service library.
- Recover exact paired history behind new durable revisions before independent approval.
- Fence accounting/publication transactions and expose only borrowed domain readiness.
- Qualify offline; add no vendor activation, daemon selector or implicit defaults.

## Context

The existing acceptance, supervisor, restore ledger and native companion APIs are
individually qualified. An operational composition must preserve their ordering:
configuration agreement is not approval, capture is not readiness, and a lost
worker cannot infer external quiescence. Partial history cannot reconstruct the
global identity ledger safely.

## Decision

Add a finite typed composition API in `services/market-data`, using existing
application/domain policies and adapter ports. Begin fresh blocked restore
revisions for every source before transient reconstruction. Inventory all retained
history within explicit aggregate bounds, including tombstones. Recover original
pending canonical/native preparations; reject incomplete, retired, expired,
legacy or excessive history. Reaccept native records under complete current
owner inputs and exact references without changing source or receipt time.

An independent owner approves each exact revision. Running workers use a fenced
ledger wrapper that enforces the revision and lifecycle phase inside existing
account/session transactions. Restoring workers can only recover an existing
identical preparation; they cannot create a new intent or charge a connection.
Existing unfenced capture APIs remain compatible and must not participate in
this runtime. Trusted principals retain responsibility for fencing prior actors.

Round-robin bounded intake charges complete synthetic frames before acceptance.
Publication futures skip busy sources, preserve unresolved queue occupancy and
confirm whole-frame groups only after every instrument pair commits. A shared
history budget reserves rows/bytes before staging all proposals and before PUT.
No reservation refund follows uncertainty. All work and final fencing have
explicit finite deadlines; no task is detached or automatically retried.

Fatal semantics/overload latch incidents. Operational uncertainty begins a new
blocked restore; failed persistence reports unconfirmed status. Consuming the
local run phase before mutation prevents reuse after future cancellation/drop.
No connection release follows worker loss; independent confirmed quiescence is
required. Successful execution alone is insufficient: recheck actual authority,
original receipt lifecycle and exact descriptors, then apply domain readiness.
Return borrowed point-in-time evidence, never an activation capability.

## Alternatives considered

- Compose only a scheduler: leaves persistence, recovery and readiness ordering unproved.
- Rebuild only a recent suffix: loses global duplicate/conflict and predecessor history.
- Cache approved digests or mutate receipt timestamps: invents authority or freshness.
- Spawn reconnecting live workers: exceeds the offline scope and introduces hidden quotas.

## Consequences

The service owns composition while domain policies remain framework-independent.
Complete history and explicit bounds trade unbounded uptime for safe finite runs.
Retirement may require owner reconciliation/new admission. Supplied trusted
catalogs are snapshots; external reference changes require owner denial/fencing.
The API has no CLI mode; synthetic integration tests are its executable entrypoint.

## Compatibility and migration

No schema or public wire-format changes. Additive APIs include fenced ledgers,
metadata-first history inventory, native replay and grouped supervisor dispatch.
Existing low-level capture callers retain their semantics and do not gain
readiness. All actors sharing a runtime admission must use fenced APIs. Rollback
must preserve durable barriers, exact intents and unresolved external capacity.

## Security and operations

Independent owner and restricted worker LOGINS remain distinct. No provider keys,
network selection, paid calls or model activity is introduced. Full input values
remain protected; errors/debug output omit payloads. See the
[runtime guide](../development/offline-stream-runtime.md) for explicit ceilings,
deadline accounting, revocation, restoration and point-in-time readiness limits.

## Validation

The owned `offline-stream-runtime` CI proof suite exercises independent approval,
grouped multi-instrument publication, fairness under paused I/O, exact state-loss
recovery, stale-worker rejection, sticky failure, revocation and readiness denial.
Its independent inventory prevents silent test removal. Local workspace compilation
and strict Clippy check all targets; database/storage proofs and migrated-schema
SQLx metadata freshness run in CI. No live qualification is claimed.
