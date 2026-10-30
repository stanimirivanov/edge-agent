# ADR-0098: Enforce independent Research context

- Status: Accepted
- Date: 2026-10-30
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use exact endpoint direction, closed event windows and independent freshness limits.
- Require affirmative owner-bound coverage; unknown and stale critical context abstain.
- Preserve existing constraints and staged wire/runtime compatibility.

## Context

M05 requires event, regime and evidence-quality controls. The maintainer selected an
initial exact-price per-instrument rising/flat/falling classifier, closed exclusion
endpoints and an independent age ceiling in addition to existing owner-quality checks.

## Decision

Compare exact raw endpoint closes, never strategy score/labels. Require an explicit
nonempty allowed regime set. Block events at either exclusion endpoint, including the
zero-width case. Require a complete owner calendar covering the entire closed window;
half-open source coverage must extend strictly beyond its end.

Bind context to eligible instrument and daily digest. Validate source knowledge,
retained rights and bounded sorted event census. Enforce the selected age ceiling on
latest market-event time and calendar knowledge time. Sealed evidence retains original
owner quality, completeness, chronology and basis checks. Aggregate critical context
failures deterministically across all required candidates before any narrowing.

## Alternatives considered

Rounded momentum can conceal tiny price changes and cannot establish regime authority.
Absent events cannot prove complete coverage. Receipt time cannot refresh old evidence.
A live/learned regime or event provider is outside this milestone slice.

## Consequences

Valid event/regime violations add stable narrowing reasons; missing/stale/future/invalid
critical context abstains with existing wire reasons and no apparent approval decision.
The new context input wrapper preserves previously selected liquidity and market gates.
No strategy, model or transport label overrides independent policy or account controls.

## Compatibility and migration

Existing public input structs, Research wire records and workers remain unchanged.
Selecting context makes older evaluator calls without it abstain. Full bundle loading,
provenance and mandatory publication enforcement remain later work. No storage migration
is needed; rollback removes the additive API before consumers adopt it.

## Security and operations

Caller authentication of owner inputs/configuration remains mandatory. Hashes and typed
conversion grant no authority. Positive current coverage is explicitly synthetic. Errors
omit payloads; no provider, credential, dependency or production source claim is added.

## Validation

Tests cover exact versus rounded direction, endpoint/zero windows, partial/unknown
coverage, future/stale/invalid/rights failures, independent market age, stable reason
priority, non-bypass and unchanged scores/lineage. Register the application proof in
protected discovery and run repository/architecture and full default checks.
