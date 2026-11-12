# ADR-0112: Version verified explanation preparation

- Status: Accepted
- Date: 2026-11-12
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Project explanation facts deterministically from an exact retained artifact and decision.
- Give the projection and canonical preparation separate explicit revisions.
- Preserve legacy input, rendering, companions, replay and qualification evidence.
- Authentication, independent verification and current disclosure rights remain caller responsibilities.

## Context

The legacy worker and frozen qualification driver prepare different input shapes
and identities. Richer explanations need a common fact boundary before a new
renderer and evaluator can be frozen. Silently changing existing preparation
would invalidate historical profile and companion bindings.

## Decision

Add `research.explanation-facts.v1`, a read-only projection of the bound request,
decision, approval, ordered ranks, ppm units, source digests, availability times
and expiry. Preserve the complete artifact evidence census as well as each
rank's supporting references. Exclude narrative artifact text. Do not infer missing policy
measurements or event/observation timestamps from reasons or availability time.
Only an artifact with an exact bound evaluated decision and supporting evidence
for each rank can form the projection. This is consistency checking, not proof
of owner authenticity, policy calculations or current rights.

Add `research.explanation-input.v2`: canonical facts, the validated standard
proposal, its explicit revision, and null untrusted context. Recursively sort
object keys and retain one final LF. The prepared evidence identity is SHA-256
of these exact bytes, independently of an evaluation suite's identity. No
model-supplied facts or proposal are accepted by the preparation constructor.

## Alternatives considered

Changing the legacy input in place would break retained bindings. Accepting
model-populated fact DTOs would weaken the deterministic fact boundary. Keep
both alternatives out of the new path.

## Consequences

A single bounded projection supports later renderer and evaluator consumers.
It cannot supply measurements absent from retained v1 records; those need
independently verified enrichment in later slices.

## Compatibility and migration

This additive foundation serves later renderer/evaluator and guarded runtime
slices. It adds no active profile selection, dispatch, persistence or replay
route. Existing worker inputs, v1 rendering, profiles, companions and frozen
qualification identities stay unchanged. Consumers must deliberately adopt the
new revision and obtain matching qualification before activation; they must not
store its digest through a legacy replay path. No migration is required.

## Security and operations

Construction checks consistency only. Callers retain authentication, independent
owner-evidence verification and current purpose-specific permission checks. The
API performs no I/O, emits no logs and accepts no model-provided fact values.

## Validation

Test exact retained values, missing support, foreign bindings, rank/source
order, rejected model fact fields and a pinned canonical digest under both
serde map-order configurations. Compile-fail coverage prevents deserialization
of model output into the read-only projection. Run the default repository gate.
