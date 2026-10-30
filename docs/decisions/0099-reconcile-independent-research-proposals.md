# ADR-0099: Reconcile independent Research proposals

- Status: Accepted
- Date: 2026-10-30
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Rebuild expected governed output from trusted inputs independently of proposal claims.
- Require exact canonical terminal bytes; return a sealed validated type.
- Add typed preparation while retaining staged legacy paths until mandatory enforcement.

## Context

M05 needs independent artifact validation. Structural v1 checks cannot detect a plausible
but altered score, evidence digest, expiry or renderer statement. Strategy output cannot
certify itself or choose the inputs used to validate it.

## Decision

Decode proposals with the existing bounded canonical contract. Require artifact/abstention
kind, then recompute from separately supplied admitted request, authenticated evidence,
selected policy/intent and immutable expiry. Reuse qualified deterministic kernels and
compare all canonical bytes. Qualified integer rounding already encodes numeric tolerance;
no additional score discrepancy is permitted or repaired.

Return private-constructor ValidatedOutcome with immutable accessors and recomputed decision.
Add PreparedCompletion::from_validated retaining existing delivery/request checks. Preserve
safe typed failure causes without payload diffs. Pin the validator revision for later audit.

## Alternatives considered

Trusting a supplied expected digest or strategy approval Boolean would allow self-validation.
Shape checks alone miss canonical factual mutations. Duplicating arithmetic formulas would
create drift. Relaxing rounded integer equality would change authoritative facts.

## Consequences

The typed path can prepare only independently reconciled terminal results. Critical evidence
failures cannot validate an artifact, although a correct abstention remains publishable as
an abstention. Authentication and current approval/control authority remain separate.

## Compatibility and migration

No wire, storage or existing worker changes. Legacy preparation remains during rollout;
PR12 must enforce persisted validation/control binding and PR11 adds provenance. Rollback
removes the additive path without changing historical artifacts. No dependency is added.

## Security and operations

Proposal fields never choose trusted evidence, policy or authorization. Callers authenticate
separate input/configuration resolution. Errors reveal no raw proposal or expected/actual
differences. No model, provider, broker or account-risk capability is introduced.

## Validation

Canonical mutation tests cover scores, identities, ranking, evidence, expiry, explanation
and terminal kind. Boundary tests cover missing selected evidence, safe causes, foreign
delivery, exact bytes, narrowed/abstention outcomes and compile-fail type separation. Register
the application proof in protected discovery and run the complete default gate.
