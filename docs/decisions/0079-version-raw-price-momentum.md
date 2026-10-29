# ADR-0079: Version raw-price momentum

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Compute exact endpoint raw USD price return over sealed eligible sessions.
- Publish a signed integer ppm score with nearest ties-to-even rounding.
- Pin formula revision and preserve lookback/session metadata; add no runtime activation.

## Context

M04 needs one deterministic momentum calculation before candidate ranking. The
existing wire contract represents scores as signed 64-bit integer ppm, while
research-domain already gates complete historical raw USD evidence.

## Decision

Revision `raw-usd-roc-ppm-half-even-v1` computes `(last/first - 1) * 1000000`,
rounded once to nearest integer with exact half ties to even. Successful scores
have at most half a ppm absolute error. Negative and zero returns remain values;
strategy eligibility is deferred. Lookback is the number of selected session
intervals, not elapsed calendar days or a holding horizon. Preserve both endpoint
sessions and basis in the result. Only sealed eligible input may be calculated.

Reuse exact USD coefficients and checked integer long division. Six decimal-digit
steps avoid the unnecessary overflow of multiplying a large price delta by one
million. Apply sign after rounding magnitude. Reject scores that cannot fit i64.

## Alternatives considered

Floating point would require platform/rounding reconciliation. A new decimal or
indicator library adds dependencies for one rational formula. Direct scaled
multiplication overflows for some otherwise representable large-price returns.
Truncation or ties-away rounding would introduce different bias and compatibility
semantics. No logarithmic, volatility-adjusted or total-return indicator is added.

## Consequences

This formula does not adjust splits/dividends or qualify raw evidence across
unsupported actions. M04-PR3 retains evidence-policy authority; applications must
still authenticate and verify owner exports. The calculation produces no ranking,
strategy registry, order, scenario, projection or publication side effect.

## Compatibility and migration

No wire or persisted contract changes. Reuse the existing ppm field and USD value
type. Configuration and replay must pin this revision before consuming it. Future
formula or rounding changes require a distinct reviewed revision. No migration is
needed; reverting the additive module before consumers adopt it is sufficient.

## Security and operations

No clock, network, storage, credential, model or provider operation exists. Numeric
errors withhold price values and retained evidence. Out-of-range scores fail closed
rather than overflowing or silently clamping. No deployment or paid-resource effect.

## Validation

Protect six tests in the default research-momentum gate: exact vectors, ties,
scale/large-value equivalence, range errors, session/revision metadata and exact
rational error bounds. Run repository, architecture and full default verification.
