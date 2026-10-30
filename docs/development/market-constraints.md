# Volatility and opening-gap policy

## TL;DR

- Use population SD of rounded simple interval returns, rounded upward to ppm.
- Use maximum absolute open/prior-close gap, rounded upward to ppm.
- The shared lookback+1 census, exact evidence binding and inclusive maxima are explicit.

## Formula revisions

For L+1 selected raw closes, compute L simple close-to-close returns using the existing
momentum ties-even integer-ppm rounding. `population-sd-rounded-return-ceil-ppm-v1`
returns the ceiling of their population standard deviation. It is not sample SD,
log-return volatility or an annualized/forecast value. L=1 has population SD zero.
Rounding each interval is part of this revision, not an approximation silently introduced
by the implementation. It measures the dispersion of those qualified rounded returns.

For each later session, compare its raw open with the immediately preceding selected
close. `max-selected-opening-gap-ceil-ppm-v1` returns the maximum absolute fractional
gap, rounded upward to ppm. The earliest selected close is the predecessor anchor;
there is no inferred gap for the earliest selected open. The entire declared opening
census is nevertheless required. No preceding calendar day or weekend is guessed.

The [calculator](../../crates/research-domain/src/market_constraints.rs) uses integer
long division for gaps and reuses unchanged momentum rounding for returns. Population
variance is `sum(i<j, (r_i-r_j)^2) / L^2`, avoiding subtractive cancellation. Integer
binary search finds the smallest u32 k with `(k*L)^2 >= sum`, so no variance floor or
floating-point square root changes boundaries. Both metrics must fit u32 ppm and each
interval return must fit the existing i64 return profile; otherwise return a typed
error. A checked squared-sum overflow also implies an unrepresentable u32 result for
L <= 252. There is no clamp, saturation or unchecked ceiling increment.

## Evidence and enforcement

`OwnerLoader::policy_opens` converts an authorized [policy companion](policy-evidence.md)
into raw opens and explicit action flags. Conversion alone grants no authority.
Calculation binds instrument, retained daily digest and exact selected sessions to
sealed eligible closes. Duplicate, foreign or rebound rows reject; missing rows or
predecessors abstain. Any selected corporate-action annotation is unsupported by these
raw revisions and fails closed, even for an otherwise excluded candidate. The existing
evidence gate also rejects unsupported adjustment basis and failed owner quality.

Select both exact revisions and explicit inclusive maxima with `with_market_constraints`.
Zero is a real limit and never disables completeness. Use `ConstraintInputs` with
`evaluate_with_market_inputs`; it also carries volume when liquidity is selected.
Older evaluator entry points supply no opens and therefore cannot bypass a selected
gate. Check all required candidates before narrowing. Missing history maps to the
existing insufficient-history abstention, invalid inputs to invalid-evidence, and
arithmetic failure to an error with no partial output. Threshold denials append
`volatility_exceeded` then `gap_exceeded` after earlier reason priorities. Narrowing
preserves scores, ordering, request and complete evidence census. These are eligibility
checks only, not strategy approval, publication permission or account risk.

## Verification and rollout

Run `cargo test --locked -p edgeagent-research-domain -p edgeagent-research-application`,
`cargo test --locked -p edgeagent-research --test policy_evidence`, and the default gate.
Tests cover independent small integer oracles, constant history, sub-ppm ceilings,
representable limits/overflow, exact threshold equality, narrowing, missing/duplicate
and changed-digest inputs, action refusal and older-entry-point non-bypass. Adapter
tests preserve raw opens/action annotations. See [ADR-0097](../decisions/0097-enforce-volatility-and-opening-gap-limits.md).
No dependency, wire/schema or runtime activation changes. Full bundle loading and
mandatory publication enforcement remain later work. Rollback removes these additive
gates before adoption; existing artifacts need no migration.
