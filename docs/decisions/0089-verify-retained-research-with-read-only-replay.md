# ADR-0089: Verify retained Research with read-only replay

- Status: Accepted
- Date: 2026-10-30
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Recalculate retained Research from its original owner-authorized frozen evidence.
- Verify original input/receipt integrity in a read-only transaction.
- Compare exact bytes without publishing or reporting current read failure as success.

## Context

The worker recovers an existing receipt before evidence reads, correctly preserving
idempotent completion after owner unavailability. Recovery is not a reproducibility
proof and can register an additional inbox delivery. Replay must verify the actual
original input and recalculate without those writes or a second artifact.

## Decision

Add explicit local `offline-replay` using the existing scope/owner profile and
original retained incoming envelope. A separate read-only owner lookup verifies
completion/outbox integrity, the exact original inbox receipt and publication
causation/correlation. Missing or inconsistent state refuses replay; it is never
created or repaired by the command.

The application verifier accepts no publication/store capability. Require explicit
renderer/momentum names matching compiled constants and existing operator-reviewed
implementation/configuration bindings. Reload authorized retained evidence, verify
exact digest/descriptor, recompute deterministic policy/momentum/ranking/rendering
and require original bytes. A current read error refuses Ready-input replay, even
when its classification matches a historical abstention. Retained owner-declared
abstentions and legacy direct-evaluation policy outcomes can be reproduced directly.

Reuse existing receipt, loader, numeric/rendering and canonicalization patterns.
No new external dependency, arbitrary path, mutable expected-output claim, schema
or replay publication channel is introduced. The reviewed profile remains the
provenance authority; automatic binary measurement/orchestration is not supplied.

## Alternatives considered

Calling worker recovery can return first bytes without recalculation and mutate
inbox state. Publishing into the normal completion path would mix verification
with live intent. Copying a current read-failure reason would certify a different
historical event. Arbitrary evidence-file access could bypass owner rights and
retention; current authorized owner reads preserve that boundary.

## Consequences

Successful replay demonstrates byte reproducibility under declared retained inputs
and reviewed revisions, not current validity, entitlement or historical binary
attestation. Erased, expired or revoked evidence may make replay unavailable.
Keep original inputs and integrity proofs for replay, not only terminal bytes.

## Compatibility and migration

This is an additive explicit mode and read-only lookup. Existing descriptors,
worker/relay, contracts and schemas remain compatible. Stop replay processes to
roll back the binary; no replay-owned persisted state or migration requires undoing.

## Security and operations

Continue the trusted loopback local operator profile. Scope supplied in input
cannot grant authority. All external reads occur after the read-only transaction
ends and retain existing deadlines, payload bounds and current authorization.
Errors preserve safe typed causes and omit payload, SQL and credentials.

## Validation

Protect pure replay/permutation, revision/binding/output refusal, owner-declared
abstention and current read-failure tests. Real PostgreSQL and independent command
processes prove repeat/restart equality, missing/changed/corrupt inputs, unavailable
reads and unchanged full completion/lifecycle/inbox/outbox snapshots. Complete
four-process qualification remains PR15, rather than being inferred from these tests.
