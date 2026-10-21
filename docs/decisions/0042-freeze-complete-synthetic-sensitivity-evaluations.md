# ADR-0042: Freeze complete synthetic sensitivity evaluations

- Status: Accepted
- Date: 2026-10-21
- Milestone: M08 - Durable projections and evaluation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Freeze one chronological input universe and all sensitivity cases before evaluation.
- Reuse P05 continuations, P09 economics, P10 settlement and P12 owner exports.
- Report exact closed-account outcomes, never invented open-position valuations.
- Synthetic determinism and sensitivity are not empirical market fidelity.

## Context

The simulator already retains explicit economics and recoverable account state.
Comparisons must not silently change evidence, omit unfavorable cases, mark open
inventory at cost or treat repeated revisions as independent statistical samples.

## Decision

Add a pure audit-domain evaluation consumer. An immutable versioned plan binds
the frozen manifest, selection cutoff, inclusive holdout and complete ordered
baseline/cost/latency/liquidity census. Require selection strictly before the
holdout and the entire manifest inside it. Case profiles keep the same semantic
model revision and change exactly their declared factor group. Every numeric
assumption, including zero, remains explicit.

Authenticate owner exports and their same-commit run checkpoints outside the
pure API. Check complete contiguous owner revisions, expected high-water, account
and funding equivalence, original configuration/plan bindings, checkpoint
identity, nonregressing cursor/time, no future valuation and exhausted frozen
inputs. Reject incomplete histories and terminal open positions/orders/holds.
Replay uses the existing P12 reducer; it does not execute financial effects.

Publish canonical versioned JSON with ordered case and trace identities, exact
realized net/fees, separate share/contract fills, transition/order counts,
absolute fee-exclusive cash flows (including settlement), cash-plus-book-cost
drawdown and paired baseline deltas. Explicitly label uncertainty unestimated
for one synthetic path. No MTM, annualization, significance or winner selection.

## Alternatives considered

- Recalculate financial authority inside a projector: duplicates domain logic
  and bypasses owner history. Test production calculators; reconcile exports.
- Guess marks for open inventory: creates unsupported P&L. Reject instead.
- Compare only successful or best cases: permits cherry-picking. Require census.
- Adopt a statistical framework now: no independent empirical population yet.

## Consequences

The offline consumer has no effects or new deployable. Bounded plans contain
4..=16 cases, each with 1..=8192 transitions, at most 16 MiB encoded history and
64 KiB output. These are safety bounds, not throughput targets. Callers retain
all original artifacts; hashes alone do not prove experiment registration time,
authenticity, representative costs, calendar membership or provider quality.
Execution profiles gain read-only accessors; the audit domain depends inward on
the simulation and market domains. No SDK or persistence dependency is added.

## Compatibility and migration

No existing wire, database, manifest or checkpoint encoding changes. New plan
and report versions are independent artifacts. Retain completed reports rather
than rewriting old experiments. No migration or runtime rollout is required;
rollback stops creating new reports and preserves existing evidence. Public
transport/storage/deployment and a qualified historical runner remain separate.

## Security and operations

Authenticate and fence export/checkpoint/registration storage. Treat reports as
account-sensitive data; Debug summaries redact histories. Checksums detect
corruption, not malicious rewrites or unauthorized rollback. Freeze population,
strategy, reference/calendar/corporate-action/rights and code artifacts in the
manifest configuration; verify retained bytes before domain use. The evaluator
cannot verify private configuration semantics or certify provenance from hashes.
No provider, model, broker, database or network operation occurs here.

## Validation

`cargo xtask verify` includes six protected `frozen-evaluation-contracts` proofs
and independent census mutation checks. Invented option snapshots exercise
production fill/fees/virtual latency/participation and cash-settlement calculators;
straight and checkpoint-resumed paths produce identical reports. Negative tests
reject omitted/multifactor cases, overlapping holdouts, future marks, corrupt or
incomplete continuations, foreign/conflicting funding/history and open exposure.
These qualify finite synthetic fidelity, not all M08, provider realism, durable
evaluation publication, statistical calibration, HA or load behavior.
