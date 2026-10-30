# ADR-0096: Enforce exact close-volume liquidity

- Status: Accepted
- Date: 2026-10-30
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Version the maintainer-selected close-volume mean over selected lookback+1 sessions.
- Floor once after averaging to whole cents, with an inclusive explicit minimum.
- Require complete owner-bound volume even for zero thresholds and excluded candidates.

## Context

M05 needs liquidity constraints over the owner-exported daily evidence. The maintainer
selected a close-price proxy, the shared momentum census and one final conservative
floor rather than an independent liquidity window or actual traded notional claims.

## Decision

Use exact raw USD close times reported shares, summed before division by session count
and one floor to cents. Reuse fixed-scale quantity types and checked i128 intermediates,
returning u64 cents. Nonnegative inputs and the bounded census guarantee an overflow
cannot conceal an otherwise representable result. No floats, truncation per row,
saturation or new arithmetic dependency is introduced.

Bind volume inputs to eligible instrument, evidence digest and exact session census.
Select the gate by its exact revision and explicit minimum; unknown revisions reject.
Apply the gate to all required candidates before narrowing. Zero is observed volume,
not unknown; a zero minimum does not disable completeness checks. Missing/invalid
inputs abstain, while unrepresentable arithmetic returns an error without partial output.

## Alternatives considered

Price times volume is only a proxy; true traded notional requires different evidence.
Per-row rounding distorts the mean. Inferring missing volume as zero, dropping invalid
excluded candidates or omitting the gate on an older evaluator path would weaken policy.

## Consequences

Liquidity failures extend deterministic reason precedence and can narrow ranking without
rescoring or rewriting source lineage. These checks do not certify final publication,
strategy approval, fill participation or account risk. Full bundle loading remains later.

## Compatibility and migration

Eligibility-only policies and current workers retain staged behavior. Selecting liquidity
requires inputs at either governed entry point. Existing wire records and retained state
are unchanged; no migration is needed. Rollback removes the additive gate before adoption.

## Security and operations

The owner loader authenticates evidence and checks rights. Typed conversion/hashes alone
grant no authority. Errors omit evidence values and preserve typed causes. No new
provider, credential, deployment boundary, dependency or market-data claim is introduced.

## Validation

Independent exact vectors and an integer oracle cover arithmetic and one-floor semantics.
Tests cover identity/session mismatch, missing data, zero threshold, equality, narrowing,
older-entry-point refusal and unchanged scores/lineage. Run required default checks.
