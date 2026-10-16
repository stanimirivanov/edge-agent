# Outbox relay refactoring plan

## TL;DR

- Milestone: **M02 - Contracts and event spine**.
- First secure telemetry and isolate observation from durable resolution.
- Then enforce a recovery-aware publication budget with real recovery evidence.
- Finally make configuration and runtime failure contracts explicit.
- Keep each slice independently reviewable; GitHub issues and pull requests own
  delivery status, while accepted ADRs own durable decisions.

## Outcome and invariants

The relay must expose trustworthy observations without hiding the ordering of
claim, validation, external publication, and one fenced durable resolution.
Keep the public crate-root facade, inward production dependencies, one in-flight
publication, immutable identity and content, and the original claim generation.
No cancellation or returned persistence error may start an alternate transition
or report an unconfirmed durable outcome as success.

The [relay guide](../outbox-relay.md) describes current behavior.
[ADR-0009](../../decisions/0009-fence-outbox-transitions-by-claim-generation.md)
governs fencing and
[ADR-0019](../../decisions/0019-validate-outbox-timing-at-the-port.md)
governs timing and the current configuration API.

## Ordered slices

1. **Secure relay telemetry and separate observation from outcome resolution**
   ([issue #174](https://github.com/stanimirivanov/edge-agent/issues/174)).
   Construct diagnostic identity only after complete stored-record validation;
   rejected records have no identity fields. Extract private observation and
   resolution modules, remove repeated registry validation, and protect an
   isolated emitted-event and returned-storage-failure matrix through the
   existing T2 runner, discovery, and routes. Preserve retry and error semantics.
2. **Enforce a recovery-aware publication-attempt budget**
   ([issue #176](https://github.com/stanimirivanov/edge-agent/issues/176)).
   Gate publication before external I/O: the final allowed attempt may publish,
   while a recovered claim beyond the budget may not. Record an ADR distinguishing
   claims from physical publication requests and introduce truthful policy
   exhaustion rather than fabricating a publisher failure. Prove boundary cases,
   current-claim quarantine, real PostgreSQL cancellation, and producer-result
   loss using the existing event-spine suite and real JetStream publisher.
   Holding a confirmed receipt proves application result loss, not wire ACK loss.
   [ADR-0022](../../decisions/0022-bound-relay-publication-by-acquired-claims.md)
   records the claim-count budget and exhausted-recovery tradeoff.
3. **Make relay configuration and runtime failure contracts explicit.**
   After the first two slices, introduce validated attempt/backoff configuration,
   accept the existing typed lease, and separate startup policy errors from
   runtime availability, state-transition, and invariant failures. Preserve the
   storage cause plus separately inspectable triggering failure. Type calculated
   retry delays without silently replacing invariant failures with a fallback.
   Supersede the relevant ADR-0019 constructor choice; migrate all callers and
   exhaustive matches together without parallel legacy constructors.

## Compatibility, rollout, and rollback

Slice one is a private responsibility split plus an additive identity-free
telemetry constructor. Its intentional diagnostic change removes correlation
from rejected records. Retained quarantine evidence remains the authorized
inspection path. Event bytes, database schemas, reason codes, and accepted retry
timing remain unchanged.

Slice two adds public failure/reason variants and the bounded
`publication_budget_exhausted` code without event or database schema changes.
ADR-0022 defines exhaustive-match migration, acquired-claim meaning, uncertain
earlier effects, mixed-worker rollout, and rollback risk. Slice three may change
configuration types and runtime error contracts; its issue and ADR must define
compatibility before implementation. Do not tighten decoder
compatibility or change the existing unknown-type quarantine policy incidentally.
Rolling back the first slice restores the telemetry exposure; prefer a forward
fix that preserves the trust boundary.

## Verification and exclusions

Run both repository sensors, both architecture sensors, and
`cargo xtask verify` for each slice. Keep existing cancellation, redaction,
retry, and same-owner generation-fencing tests intact in meaning. Extend the
existing protected T2/T3 inventories and route fixtures rather than adding a
parallel runner. Report service-backed checks separately from discovery.

Do not introduce a worker runtime, exporter installation, active lease renewal,
operator APIs, generic error macros, a shared testkit framework, or model calls.
Tests use synthetic evidence. No slice creates a live trade-execution path.
