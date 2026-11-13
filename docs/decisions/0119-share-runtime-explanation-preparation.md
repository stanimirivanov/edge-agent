# ADR-0119: Share runtime explanation preparation

- Status: Accepted
- Date: 2026-11-13
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Share one production preparation/rendering implementation with a new frozen suite.
- Preserve historical inputs, suites, ratings and approval eligibility.
- Land this prerequisite before guarded worker/launcher composition.

## Context

PR31's complete qualification protocol includes invented fixture assertion templates
and a benchmark presentation revision, while the worker accepts legacy preparation
and renderer pins. Treating those as runtime parity would change qualification
meaning. The maintainer approved a separate parity prerequisite before M06-PR32.

## Decision

Add `RuntimePreparation`, a pure constructor over independently authenticated
Research records, decisions, execution configuration and owner execution evidence.
Reuse the M05 policy reconciliation, canonical verified facts and strict typed
proposal validator. Serialize `research.explanation-input.v5` with only canonical
prepared facts, a deterministic policy baseline and optional purpose-authorized
reviewed context. Exclude fixture-only assertion templates from model input.
Require the existing 65536-byte limit on the complete canonical envelope plus LF.

Use `research-explanation-runtime.v1` for the shared presentation and preserve the
allowlist validator revision. Bind profiles to those exact pins and bind evidence
to the actual prepared bytes. Models choose typed presentation variants; they
cannot supply measurements, chronology, rankings, source facts or disclosures.
Retain an immutable sealed typed validation alongside the new deterministic text.
No inference or publication authority is part of this pure API.

Optional excerpts reuse the existing checked model-disclosure preparation.
Rendering requires fresh, separately purpose-bound owner and Research serving
observations for both artifact and document. Expose the existing serving check
as a checked document accessor, preserving historical reviewed-context bytes.
Append the canonical document as quoted data without duplicating legacy preview
text or accepting its instructions as claims or tool authority.

Add `Suite::runtime()` and the v3 manifest. All thirty original tasks use the
shared production constructor and renderer. Preserve semantic guards, two
independent 21-of-30 thresholds, zero preference credit for fallback/failure/ties
and all 47 deterministic safety checks. Preserve all old suite APIs/identities.
Freeze new case/presentation/suite identities over actual shared preparation,
rendering, fixture evidence and assertions used only by semantic guards. Prepare
the four existing adversarial contexts through the same reviewed-document path;
include their exact input identities in the new suite pin.

Expose `runtime_protocol()` and runtime probe inputs for later explicit producer
and worker adoption. The new preparation census uses a v2 domain separator;
historical PR31 `protocol()` and approval decoding remain unchanged. An old
approval cannot be relabelled to authorize this renderer or preparation.

## Alternatives considered

Copying invented fixture templates into production or changing existing suite
pins would conceal a different evidence claim. Keeping parallel preparation and
rendering implementations would leave the parity gap unresolved. Activating a
worker while merely refusing all approved profiles would not deliver generation.

## Consequences

The pure path is usable by both qualification and later production orchestration.
This is an unactivated foundation: no real model is qualified or enabled, no
producer/launcher changes ship, and no owner approval is automatically migrated.
PR32 must explicitly adopt versioned approval/producer, worker, replay and serving
bindings before claiming guarded generation or qualification/runtime parity.

## Compatibility and migration

No persistence migration, event/companion rewrite, dependency or profile update.
Existing worker, replay, queries, v1/v2 suites and private evidence stay on their
historical paths. New helpers require fresh exact artifact pins and matching
qualification evidence. Rolling back this additive path does not alter history.

## Security and operations

Current owner authentication, actual prepared-byte/runtime verification and fresh
rights checks remain caller responsibilities before dispatch. Prepared snapshots
and frozen fixture grants confer no current authority. No raw licensed data,
private manifests, weights or rejected model responses are added to the repository.

## Validation

Assert byte/text parity for all thirty tasks, semantic guards, unchanged historical
identities, new frozen identities, fresh presentation bindings and unchanged
thresholds. Exercise expired/revoked/wrong-purpose context and an oversized combined
input whose constituent policy/context inputs are valid. Run mutation controls,
both map-order modes, repository/architecture checks and the default gate.
