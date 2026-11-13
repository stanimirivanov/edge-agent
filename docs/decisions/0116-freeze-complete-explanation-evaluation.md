# ADR-0116: Freeze complete explanation evaluation

- Status: Accepted
- Date: 2026-11-13
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Freeze a new complete thirty-case census with unchanged reader questions.
- Require semantic guards and exact input/presentation pins before scoring.
- Preserve v1 identities, historical ratings, thresholds and disabled generation.

## Context

Historical v1 has ten executable cases and twenty unresolved tasks. A matching
fixture digest alone cannot establish that a scenario means what its question
claims. PR23–26 provide verified preparation, policy measurements, chronology
and reviewed context; a new revision can now preserve every original task.

## Decision

Expose `Suite::complete()` alongside unchanged `Suite::frozen()`. Construct all
thirty invented cases through full M05 execution validation, then check their
actual rankings, assessments, measurements, boundaries and source attribution
against each scenario. U25 requires several claims from one source; U26 requires
different sources for distinct adverse comparisons. Wrong scenarios refuse
before pin validation. No production evidence or owner permission is implied.

Freeze case identities over the task, exact assertions/configuration, prepared
input, retained records, baseline, standard proposal and alternate presentation.
The v2 manifest preserves v1 scenario/question text. The suite pin includes the
new rubric, renderer/preparation protocols, safety protocol and legacy identity.
U29 adopts PR26's document preparation and reader-case permission/presentation pin.

Other cases use `research.explanation-input.v4`: unchanged v2 preparation plus
canonical invented assertions and the deterministic policy baseline. The common
presentation states the configured equal-score tie-break. Reuse the Contracts
canonical encoder, exposed additively without changing its existing bytes.

Keep two independent 21-of-30 thresholds, zero safety failures and zero preference
credit for ties, fallback, failure or rejection. New presentations bind exact
outputs, deployment and blind order; old ratings cannot be reused. The safety
census retains the original seventeen probes and adds a positive/foreign-citation/
missing-disclosure check for every new case.

## Alternatives considered

Replacing v1 in place would invalidate history. Renaming old recipes as ready,
dropping reader tasks or merely refreshing hashes would hide semantic gaps.

## Consequences

The complete corpus is testable without a model or services. A complete census
is not a passing real-model qualification. Reviewer identity/independence and
pinned deployment evidence remain externally authenticated responsibilities.

## Compatibility and migration

Existing driver and production behavior stay on their historical paths. Later
consumers must explicitly adopt complete prepared inputs and matching protocol
pins. No database migration, runtime approval or automatic activation occurs.

## Security and operations

Promote existing MarketData value types and serde_json 1.0.151 from dev to normal
application dependencies for pure fixture construction/encoding. serde_json's
MIT OR Apache-2.0 license is already approved; no package versions or transitive
libraries are added. Both dependency policies retain inward-only boundaries.
Synthetic grants are private fixture assertions, never production authority.

## Validation

Run the complete and historical evaluator tests under both map-order modes,
semantic mismatch controls, census/stale-presentation/threshold tests, mutation
checks, repository/architecture/default verification and applicable dependency
checks. The focused harness ID is `explanation-complete-evaluation`.
