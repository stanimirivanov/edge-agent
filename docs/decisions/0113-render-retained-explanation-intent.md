# ADR-0113: Render retained explanation intent

- Status: Accepted
- Date: 2026-11-12
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Add an explicit v2 deterministic preview of retained intent and chronology.
- Keep required limitations fixed in every presentation variant.
- Preserve legacy rendering, preparation, replay, companions and evaluation pins.

## Context

The legacy tracer exposes ranks, evidence references and aggregate reasons.
PR23's sealed facts also contain direction, intended horizon, historical
lookback, universe, cutoff and expiry. Readers must be able to distinguish
these concepts without asking a model to supply their values.

## Decision

Add `research-explanation-trace.v2`. Render the exact retained intent and
chronology with artifact, decision and source citations. Evidence age is the
nonnegative difference between decision cutoff and availability, in milliseconds;
it is not current age or a claim about a configured freshness ceiling. Event
and observation times are explicitly unavailable because v1 retained facts do
not carry them. Do not substitute availability time for either.

Reuse the validated v1 rank/reason presentation templates and research-only,
no-trading-authority disclosure. Append fixed historical-evidence/not-current-
quote and unqualified-scenario/risk-illustration disclosures. Typed proposals
can vary supported rank presentation only; they cannot omit this metadata or
these disclosures. The v2 preview returns text, not a persistable v1 companion.

## Alternatives considered

Changing v1 text would break historical byte bindings. Inferring chronology,
freshness limits or trade authorization would invent facts. Leave both out.

## Consequences

The preview exposes existing facts without changing policy or ranking. Missing
owner-backed measurements and chronology remain visible limitations; later
slices must supply verified enrichment before corresponding reader tasks can
qualify. The frozen evaluator is not revised here.

## Compatibility and migration

Expose `offline-explain-v2` through the existing authenticated completion and
independent owner replay path. Keep `offline-explain` and explanation replay
unchanged. No migration or generation activation occurs. Adopting the renderer
in profiles, canonical preparation or persisted companions requires later
explicit revision-aware integration and matching qualification.

## Security and operations

The local operator tracer confers no model-disclosure or serving authority.
Failed owner reads and inconsistent retained state refuse with empty stdout.
No model call, publication, lifecycle mutation or credential change is added.

## Validation

Test complete intent, availability-age arithmetic, explicit missing chronology,
both presentation variants, required disclosures, malformed proposals and
repeatable typed-choice replay. Exercise both CLI versions against real
PostgreSQL, including refusal and unchanged retained-row snapshots.
