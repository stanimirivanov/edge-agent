# ADR-0019: Validate outbox timing at the port

- Status: Accepted
- Date: 2026-10-09
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Require validated `LeaseDuration` and `OutboxRetryDelay` values at both the
portable outbound storage port and the PostgreSQL adapter. Preserve existing
timing semantics: leases accept 1 millisecond through 15 minutes; outbound
retries accept zero through 24 hours. Broker redelivery keeps its distinct,
nonzero `RetryDelay` contract.

## Context

The outbound port accepted arbitrary `Duration` values. PostgreSQL alone
bounded them, while relay policy duplicated the lease and retry maxima.
Alternative storage adapters could therefore accept a different range, and a
caller could discover a configuration defect only during storage work.

The existing outbox permits immediate retries and sub-millisecond delays.
PostgreSQL floors accepted durations to whole milliseconds. Reusing the broker
redelivery type would reject these currently valid outbound requests.

## Decision

Introduce private-field, copyable `LeaseDuration` and `OutboxRetryDelay` values
in the messaging crate's outbound capability. Fallible constructors and
`TryFrom<Duration>` validate inclusive bounds. `get()` and conversion back to
`Duration` preserve the exact input; equality, ordering, and hashing use it.
`OutboxTimingError` classifies caller validation failure before storage I/O.
`OutboxRetryDelay::IMMEDIATE` explicitly names zero-delay eligibility.

Require these values in `OutboxRelayStore::claim_one` and
`release_for_retry`, and in the concrete PostgreSQL claim and retry methods.
Remove adapter-local copies of the timing policy. The adapter retains checked
integer conversion and its existing whole-millisecond flooring. A positive
retry below 1 millisecond therefore remains immediately eligible after commit.

`RelayPolicy::new` retains its raw configuration inputs, validates through the
portable types, and keeps application-specific constraints: base retry is at
least 1 millisecond and maximum retry is at least the base. The coordinator
validates its calculated jittered delay before requesting a retry transition.
The public relay outcome continues to expose a `Duration` projection.

## Alternatives considered

### Keep primitive durations and adapter-only validation

- Benefits: no Rust API migration.
- Costs and risks: adapters can disagree; invalid durations reach storage work.
- Reason not selected: the application-owned port must establish its bounds.

### Share broker `RetryDelay` with the outbox

- Benefits: one retry type.
- Costs and risks: rejects immediate and sub-millisecond outbound retries.
- Reason not selected: broker disposition and outbound availability have
  different existing invariants.

### Round or reject fractional milliseconds in constructors

- Benefits: matches PostgreSQL precision directly.
- Costs and risks: loses caller precision or tightens accepted inputs, coupling
  a portable value to one persistence implementation.
- Reason not selected: this refactor preserves the accepted duration range;
  adapter precision remains explicit at the storage boundary.

## Consequences

### Positive

- Invalid timing cannot cross the safe outbound storage API.
- Relay and storage share bounds without duplicate policy constants.
- Immediate retries remain explicit and supported.
- Boundary, precision, and port-forwarding tests run without PostgreSQL.

### Negative

- Rust callers must construct validated timing values; adapter implementations
  must update their method signatures.
- Similar retry names represent intentionally different capability semantics.

### Neutral or follow-up

- No event bytes, schema, SQL statement, lease fence, or retry algorithm changes.
- Settlement ownership and port cancellation remain separate review findings.

## Compatibility and migration

All workspace callers migrate in one change; no parallel primitive entry points
remain. These unpublished workspace APIs change at compile time. Existing
database state and deployed timing configuration need no migration. Rolling
between binaries built before and after this change does not alter persisted
timing behavior or require a coordinated data rollout.

## Security and operations

Validation errors contain fixed text and no payload or provider data. A typed
lease still must exceed the publication timeout selected by the composition
root; range validation alone does not enforce that relationship. Worker count,
attempt budget, fencing, and at-least-once recovery stay unchanged. Immediate
retry support in storage does not change the relay's nonzero backoff policy.

## Validation

T2 tests cover both inclusive limits, adjacent invalid nanoseconds, overflow-
sized input, exact duration round trips, ordering, hashing, and auto traits.
Compile-fail doctests reject raw durations passed to the port. Relay fakes
assert the exact typed lease and calculated retry delay forwarded to storage,
including bound-edge policies and capped jitter. Adapter tests prove checked
millisecond conversion and flooring. PostgreSQL conformance retains the
existing immediate-retry eligibility and stale-claim fencing checks.
