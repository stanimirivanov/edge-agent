# ADR-0100: Project versioned Research decision provenance

- Status: Accepted
- Date: 2026-10-31
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Preserve immutable Research v1; attach a bounded `research.decision.v1` companion.
- Observe policy eligibility separately from approval and critical-evidence abstentions.
- Replay authenticated provenance with exact outcome binding and explicit missing-input gaps.

## Context

M05 needs retained independent policy and validation observations without changing strict
historical Research records. Consumers must understand additive provenance before PR12
begins producing and atomically retaining it. Legacy absence cannot imply approval.

## Decision

Use the existing canonical JSON, versioned DTO and event-reducer patterns. Required fields
bind the full request (including implementation/configuration/cutoff), exact terminal digest,
policy digest, explicit direction/holding horizon, approval-profile digest and observed
approval revision, validator revision and assessment. Evaluated assessments retain eligible
identities, status and all stable ordered reasons; critical-evidence abstentions use an
explicit not-evaluated reason. There is no fabricated empty successful decision.

Validate bounded shape and exact terminal consistency. These are observations, not proof of
formula correctness or current authority. The caller must authenticate owner provenance.
Policy-approved means only eligibility; account risk and strategy approval remain distinct.

The pure Research projection retains canonical companion bytes separately. Missing base
outcomes return Gap; foreign anchors, inconsistent terminal bindings and competing companions
reject without mutation. Exact duplicate companions do not advance revision. Lifecycle and
provenance commute after their common immutable outcome. The local view revision counts
observed terminal, lifecycle and provenance facts; it is not an owner or broker cursor.

## Alternatives considered

Adding fields to Research v1 breaks strict old readers and artifact digests. Inferring
approval from missing provenance invents evidence. Recomputing formulas in a projection
would move owner authority into a disposable view. A generic provenance framework adds
unnecessary abstraction; the existing canonical Serde/SHA-256/reducer pattern needs no dep.

## Consequences

Consumers can distinguish historical absence, evidence abstention and evaluated policy
results while preserving exact terminal bytes. Contract checks do not verify that the
claimed policy, validator or approval record actually ran; PR12 must bind sealed validation
and current owner authority inside the terminal transaction.

## Compatibility and migration

No producer, route, SQL, HTTP or runtime activation in this consumer-first slice. Existing
workers and queries retain their behavior. EARP1/EARP2 bytes remain exact when provenance is
absent; EARP3 carries it with an integrity checksum and a larger bounded capacity. Old
binaries reject EARP3. Upgrade readers before producer activation; rollback after activation
requires stopping consumption/serving and rebuilding disposable views from retained owner
facts with a compatible reader. Never discard provenance to make a downgrade appear valid.

## Security and operations

Canonical hashes/checkpoints are not authentication, rollback protection or current
entitlement. Authenticate sources and atomically persist checkpoint/inbox/progress in the
adapter. There is no account, broker, model or provider access, and no payload-bearing error.

## Validation

Frozen companion and original artifact fixtures, strict canonical/version/field refusal,
assessment/outcome consistency, gap/backfill, duplicate/conflict, scope, lifecycle reorder
and checkpoint corruption/version proofs run without external services. The protected
projection inventory includes the new proof; run repository, architecture and full gates.
