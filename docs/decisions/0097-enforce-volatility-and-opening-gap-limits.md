# ADR-0097: Enforce volatility and opening gap limits

- Status: Accepted
- Date: 2026-10-30
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use the maintainer-selected population/maximum-gap definitions and upward ppm rounding.
- Bind complete opening evidence and reject action-affected raw windows.
- Enforce selected gates at all governed entry points without rescoring candidates.

## Context

M05 requires qualified volatility and gap constraints. The maintainer selected population
SD of ties-even ppm simple interval returns and maximum absolute open/prior-close gap,
both over the shared selected window, with upward final rounding and inclusive limits.

## Decision

Reuse existing momentum interval rounding without changing its formula. Compute
population variance using pairwise squared return differences and exact integer ceiling
square-root search. Compute gaps using exact long division and an upward ppm ceiling.
No annualization, floating-point statistics or new arithmetic dependency is introduced.
Metrics fit u32 ppm; interval returns retain the existing i64 profile. Overflow rejects.

Bind opens and action annotations to the same instrument, retained daily digest and
selected session census as eligible closes. Require all rows, including the initial
anchor session. Evaluate gaps only for later sessions with selected prior closes.
Reject any selected action annotation and retain existing raw-basis/quality safeguards.

## Alternatives considered

Sample SD, log returns and latest-only gaps were not selected. Floating-point boundaries
and rounding variance before square root would alter decisions. Ignoring missing opens
or checking only otherwise-eligible candidates would weaken evidence requirements.

## Consequences

Stable reasons extend existing independent eligibility controls. Selected zero maxima
still require evidence. The new grouped evidence input carries both openings and volumes
so market constraints cannot silently bypass selected liquidity. Ranking narrows without
rescoring, and evidence failures expose no apparent policy approval decision.

## Compatibility and migration

Existing workers and wire records remain staged and unchanged. Older pure evaluator
entry points abstain when selected market inputs are absent. Full bundle loading and
mandatory publication enforcement remain later M05 work. No stored migration is required.

## Security and operations

Owner authorization and configuration resolution remain caller obligations. Conversion
and hashes alone are not authority. Errors omit input payloads and preserve typed causes.
No provider, credential, service or account-risk permission is added.

## Validation

Exact vectors and independent integer oracles cover formulas, ceilings and limits.
Application/adapter tests cover equality, denials, unsafe actions, missing/duplicate or
rebound evidence and non-bypass. Register the proof in protected rendering discovery
and run the required full default gate.
