# ADR-0115: Bind reviewed context qualification

- Status: Accepted
- Date: 2026-11-13
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Bind reviewed excerpts as inert data in a new canonical preparation revision.
- Check current artifact and exact document permissions separately for each purpose.
- Pin input, presentation and reader task without revising historical qualification.

## Context

The existing document boundary checks reviewed identity and permission, but the
shared preparation has no context-bearing form. Reader task U29 needs visible,
source-bound context that cannot establish ranking facts or executable authority.

## Decision

Add `research.explanation-input.v3`, nesting unchanged v2 preparation and a
separate `untrusted_context` data object. Bind the exact canonical excerpt,
digest, model audience and model-disclosure purpose. Require the exact artifact
and document permissions from both owner and Research at construction. Context
must be available at the retained decision cutoff and within existing bounds.

Add `research-explanation-reviewed-context.v1`. Recheck both artifact and exact
document serving permissions on each presentation. Keep validated facts and
all disclosures, then display canonical quoted document data under fixed wording
that it supplies no ranking/policy evidence or tool/action authority. Embedded
newlines cannot forge output rows. No retrieval or tool capability is introduced.

A reader-case integrity pin binds the canonical input, presentation revision,
exact standard baseline, reader task and serving audience/purpose. It is not a
passing qualification score, runtime approval or substitute for current rights.

## Alternatives considered

Reusing artifact-only grants would authorize unreviewed document disclosure.
Interpolating context as instructions or changing v2 bytes would weaken the
boundary or invalidate retained identities. Keep both out of the new path.

## Consequences

The invented U29 case can distinguish quoted context from artifact facts. Later
suite integration can freeze its exact input and presentation. Missing, revoked,
expired, foreign or future authority/context refuses rather than silently
producing a different baseline. Permission snapshots do not authorize later dispatch.

## Compatibility and migration

Existing preparation, renderers, companions, replay and frozen v1 cases/ratings
stay unchanged. PR27 adopts this capability into a new complete suite. No schema
migration, production profile selection, CLI or model activation is added.

## Security and operations

Callers authenticate owner assertions and current permission observations, and
recheck immediately before model disclosure and each serving operation. The pure
API performs no I/O and never logs rejected document content. Quoted context
remains untrusted even when its source and rights have been reviewed.

## Validation

Test the positive invented reader task, canonical pins under map-order features,
injected row/instruction text, separate purpose/subject rights, foreign anchors,
changed excerpts, current revocation/expiry, cutoff equality and future refusal.
Run the complete default repository gate without altering historical suite pins.
