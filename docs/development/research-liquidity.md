# Exact close-volume liquidity policy

## TL;DR

- Use `mean-selected-close-volume-floor-cents-v1` over the same lookback+1 sessions as momentum.
- Sum exact raw USD close × reported shares; average and floor to whole cents once.
- An explicit minimum is inclusive; zero volume is observed zero, while missing data abstains.

## Numeric meaning

The maintainer selected this close-price proxy, not actual traded notional, VWAP,
spread/depth or executable liquidity. For N selected sessions the result is
`floor(100 * sum(close_usd_per_share * reported_shares) / N)`. N is exactly the
existing eligible close census, from 2 through 253 observations. No inferred weekdays,
independent window or annualization is introduced. Sessions, not calendar-day gaps,
determine membership. Unadjusted prices and reported shares retain their declared basis.

The [calculator](../../crates/research-domain/src/liquidity.rs) reuses `UsdPrice` and
`ShareVolume`. Align their exact coefficients to scales 8 and 6, sum checked products
at scale 14, then divide once by N × 10^12 for cents. No row is truncated first.
Return u64 cents; never clamp or saturate. Overflow is a calculation error, not a
threshold denial. With nonnegative inputs and N <= 253, any i128 product/sum overflow
also implies an unrepresentable u64 final result: the largest representable sum is
less than (u64::MAX + 1) × 253 × 10^12, well below i128::MAX.

## Evidence and policy boundaries

`OwnerLoader::load_policy` supplies the authorized [companion](policy-evidence.md).
`policy_volumes` converts its last lookback+1 daily rows to exact typed shares, retaining
instrument and daily digest. This conversion alone grants no authority. Calculation
requires the same instrument/digest as sealed eligible closes and exact distinct session
coverage. Duplicate/foreign rows or identity mismatch are invalid evidence; omitted
rows are insufficient history. Reordered rows are allowed. Even a zero threshold
requires complete evidence. Raw zero volume is a known value, never a missing marker.

Use `Policy::with_liquidity` with the exact formula revision and explicit minimum.
Unknown revisions reject. Existing eligibility-only policies remain staged without
this selected gate. Once selected, both governed evaluator entry points require its
inputs: the older entry point abstains for missing volume rather than bypassing the gate.
The volume-aware evaluator checks all required candidates, including instruments already
excluded by other constraints. Missing history and invalid evidence abstain without an
apparent eligibility decision. Unrepresentable arithmetic returns a typed safe error.

Threshold failures append `liquidity_below_minimum` after earlier reason priorities.
Candidates passing equality remain eligible; others narrow or cause the existing
`no_eligible_candidate` abstention. Scores, ranking order, request and full evidence
census are preserved. These decisions grant no strategy approval or account authority.
See [ADR-0096](../decisions/0096-enforce-exact-close-volume-liquidity.md).

## Verification and rollout

Run `cargo test --locked -p edgeagent-research-domain -p edgeagent-research-application`,
`cargo test --locked -p edgeagent-research --test policy_evidence`, and the default gate.
Tests cover fractional means, zero, large values, checked overflow, an independent
400-case integer rational oracle, row order, missing/duplicate/foreign inputs, threshold
boundaries and preservation of output/lineage. Owner-companion conversion retains exact
reported shares. No dependency, schema, provider or runtime activation is added. Full
bundle installation and mandatory worker enforcement remain later M05 work; callers
must authenticate the selected policy/configuration. Rollback removes this additive gate
before later consumers adopt it; existing artifacts require no migration.
