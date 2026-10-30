# ADR-0088: Append and project owner artifact lifecycle

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Append explicit-time owner lifecycle changes without rewriting artifacts.
- Serialize scoped commands and commit each revision with its exact outbox event.
- Project consecutive histories, expose gaps, and refuse closed outcome retrieval.

## Context

Versioned lifecycle records exist but have no owner execution or projection. M04
requires explicit expiry, invalidation, withdrawal and correction links. Research
owns authoritative history; disposable read models cannot grant mutation authority.

## Decision

Use the existing local owner profile and explicit `offline-lifecycle` stdin command.
Published can become invalidated, expired, withdrawn or corrected. Invalidated and
expired can subsequently become withdrawn or corrected; withdrawn/corrected are
final. Require consecutive revisions and nondecreasing explicit times from decision;
expiry cannot precede the original expiry cutoff. The graph bounds history at two
changes. No scheduler or automatic restoration of eligibility is introduced.

Append immutable records and exact Research-owned CHANGED outbox events in one
transaction. Scope-wide advisory serialization protects correction-target state
without cross-service writes. Identical retries retain original publication bytes;
conflicting/illegal/stale revisions fail. A correction target must be separately
committed, intact, same scope/decision/universe/lookback, distinct request, still
published and unexpired at effective time. Implementation/configuration may differ.
Refusing already closed targets prevents cycles without speculative graph traversal.

Reuse the independent pure lifecycle policy in the owner application and audit
reducer. Keep original outcome bytes, bounded exact history and EARP1 compatibility;
new histories use EARP2. Apply checkpoint/inbox/latest applied delivery together.
Missing predecessors remain gaps: accept backfill without hiding the gap and clear
only when the retained missing event itself succeeds. Before a base anchor exists,
retain input at the caller/broker rather than invent request authority.

Projector consumers explicitly add the CHANGED subject. Internal and Gateway
metadata add optional last lifecycle fact and closed states; closed outcome GETs
return 410. Correction links are digests and never automatic redirects. Validate
all retained lifecycle inbox proofs before serving. No observation certifies global
currency or current entitlement.

## Alternatives considered

Updating published artifact bytes destroys reproducibility. Wall-clock expiry or
autonomous surveillance exceeds explicit owner scope. Unbounded lifecycle graphs
and peer storage joins are unnecessary. A correction to a closed target can create
cycles or imply revival; allowing only published targets gives a bounded invariant.
Silently consuming missing revisions would hide gaps and lose replay correctness.

## Consequences

Lifecycle histories are authoritative, immutable and retained with publication
integrity evidence. Read models remain disposable. Serialization trades per-scope
write concurrency for clear target-state fencing; all locks and operations remain
bounded. Explicit time is historical policy input, not host-clock confirmation.

## Compatibility and migration

Apply the additive owner migration before lifecycle intake; preserve completion
bytes. EARP1 empty-history checkpoints remain unchanged; old binaries cannot parse
EARP2, CHANGED or closed metadata. Upgrade projector durable filters and consumers
before relay activation, preserving progress. Stop lifecycle intake/relay before
rollback and retain authoritative histories/events for compatible recovery.

## Security and operations

Only the existing trusted local owner operator can invoke mutation; scope in stdin
cannot authorize it. No public mutation endpoint, provider/model access, scheduler
or strategy promotion is added. Original and replacement integrity is checked in
the owner schema; projector verifies its own inbox only. Errors remain payload-safe.

## Validation

Protect pure state/time/ordering and reducer duplicate/gap/checkpoint tests. Real
PostgreSQL proves owner concurrent retries, correction-cycle/target refusal,
statement/deferred-commit rollback and owner CLI restart; projector proves atomic
gap clear/backfill, closed HTTP and missing proof refusal. Real NATS proves lifecycle
ACK/projection. Gateway tests cover closed metadata and 410 without redirecting.
