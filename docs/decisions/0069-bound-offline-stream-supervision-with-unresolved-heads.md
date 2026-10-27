# ADR-0069: Bound offline stream supervision with unresolved heads

- Status: Accepted
- Date: 2026-10-27
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use explicit bounded FIFO lanes and record-level round-robin scheduling.
- Retain each dispatched head until explicit confirmation; never retry from Drop.
- Fail health on incidents/unresolved work without claiming evidence readiness.
- Keep this slice offline and transient; durable recovery and IEX composition follow.

## Context

PR19 supplies offline IEX normalization, not canonical semantic acceptance or a
streaming runtime. PR17B already owns durable preparation/checkpoint/publication.
The maintainer approved splitting PR20 into local scheduling, durable recovery
and IEX composition. Scheduling must not invent a second event bus or bypass
those existing account, evidence and authority boundaries.

## Decision

Implement synchronous application-owned scheduling over bounded canonical values.
Pin 1..=32 distinct feeds with exact epoch/provider declarations and explicit
per-source/aggregate positive capacities, each at most 4096 retained records.
Dispatch at most one unresolved head per source, scanning at most one rotation
per call. An immutable local owner witness prevents foreign confirmation.
Retain FIFO heads across dropped handles and failed confirmations. Return rejected
intake to the caller; never evict existing records. Latch overload, binding and
explicit source failures. Cancellation stops new intake/dispatch/confirmation;
unaffected lanes can drain after a source-specific incident, but health fails.
The first incident stays sticky. There is no in-memory reset or retry API.

## Alternatives considered

- One shared queue: a hot source can monopolize scheduling and retained capacity.
- Unbounded source tasks/channels: memory and uncertainty exceed declared limits.
- Pop before confirmation or automatic replay on Drop: silently loses or repeats work.
- Treat scheduling health as readiness: ignores continuity, rights, corrections and freshness.
- Durable scheduler in this slice: duplicates established publication/account ownership
  before PR20B defines persisted incidents and restart barriers.

## Consequences

Fairness is measured in scheduling calls/records, not bytes or elapsed time.
A dropped handle deliberately blocks its lane rather than silently retrying.
Consumers still need bounded task/producer composition and durable publication.
An empty healthy supervisor proves no useful market evidence exists.

## Compatibility and migration

Additive Rust library only; no persisted state, schema, wire format, dependency
or service mode changes. PR20B adds durable incidents/restore barriers and exact
publication recovery; PR20C adds IEX acceptance and actual composition. Neither
is completed by this decision, and vendor activation remains blocked.

## Security and operations

Diagnostics redact payloads and identity bindings. Canonical construction and
matching provider declarations are not authenticated semantic/rights grants.
No socket, model call, clock, secret or external service is used. Transient queues
do not survive process loss and cannot reset durable counters or free unresolved
account connections. Stop runtime use until durable recovery is composed.

## Validation

The protected `stream-supervision-contracts` gate runs invented deterministic
fairness, capacity, ownership, failure, cancellation and redaction proofs.
The default `cargo xtask verify` includes these proofs on Ubuntu and Windows.
See the [supervision guide](../development/stream-supervision.md) and the
[M03 plan](../development/plans/m03-acquisition.md) for remaining boundaries.
