# Frozen-run sensitivity evaluation

## TL;DR

- P13 compares every declared synthetic baseline/cost/latency/liquidity case.
- Freeze and retain the plan before evaluating a strictly later holdout.
- Supply authenticated complete P12 exports and same-commit P05 continuations.
- Open/incomplete runs abstain; results are exact descriptive accounting only.
- No model/provider/broker calls or empirical performance claims.

Read [ADR-0042](../decisions/0042-freeze-complete-synthetic-sensitivity-evaluations.md),
[deterministic runs](deterministic-runs.md), [execution](evidence-execution.md),
[settlement](option-settlement.md) and [owner audit](account-audit.md).

## Composition and frozen evidence

`audit_domain::evaluation::EvaluationPlan` borrows the P05 manifest and freezes
selection cutoff, inclusive holdout and 4..=16 `EvaluationCase`s. Baseline comes
first; all cost, latency and liquidity axes must occur. One-factor cases change
only their fee pair, virtual delay pair or participation ceiling respectively.
Profile semantic revision stays fixed. Configuration and execution identities
must differ between cases; no duplicate profile or post-hoc subset is accepted.
All zeros are explicit, never recommended defaults.

Cases are isolated hypothetical forks of the same logical funded account. Do
not reprovision one operational account root with different immutable
configurations; a future runner must isolate each case's owner state/storage.

Reconstruct checked manifest descriptors and verify retained artifact bytes.
Manifest configuration must pin population/universe (including delistings),
strategy, calendar, reference/corporate-action basis, rights, financial policies
and environment. The same manifest binds evidence, ordering, code and seed across
cases. Freeze case-specific complete owner configurations and execution/postlude
bindings separately. A declaration is not proof these opaque artifacts contain
the right semantics; composition must inspect/verify them. Hashes cannot prevent
someone running experiments before registering a plan. Authenticate/timestamp
append-only registration and retain every attempted experiment separately.

The entire manifest starts inside the holdout and all availability times must
fit it. Selection ends strictly before its start. There is no implicit warmup,
training, retrospective readiness rewrite or optimizer. P05 permits loading
payloads only from consumed inputs; P09 additionally rejects retroactive
pre-activation liquidity. P10 final synthetic fixings belong to the retained
postlude configuration, not an ordinary quote used as a final mark.

## Evaluation and failure semantics

For each case supply `EvaluationTrace` with genesis through every contiguous
owner revision, expected high-water, and each frame's atomically retained P05
checkpoint. The current owner export API exposes frames, not historical run
checkpoints; a production runner must retain/authenticate these together before
using this API. Do not synthesize past checkpoints from current state or grant
evaluation access to peer private tables. This PR adds a pure consumer and
synthetic qualification, not that transport/history export capability.

`evaluate` checks common account/funding/inventory/valuation/risk genesis, case
bindings, P12 hash-chain and financial reconciliation, virtual time/cursor
progress, unconsumed genesis continuation, nonfuture risk marks and final input
exhaustion at the declared end. It requires all positions closed with known zero
cost and all orders terminal with no remaining holds. Risk breach does not hide
an experiment: it remains a reported flag. Missing/foreign/conflicting history,
unknown basis, unfinished input or open exposure returns a payload-free typed
error and no report. Input limits are 8193 frames and 16 MiB encoded history per
case; output is bounded to 64 KiB. No background work or side effects occur.

Checked snapshots prove chain/accounting integrity, not that a dishonest owner
ran the declared algorithm. The synthetic fixture independently executes P09
`Order::simulate`/`LiquidityExecution`/`FillAccounting` and P10
`SettlementPolicy::calculate_account`. It is not a provider-qualified backtester
or recomputation of production journals from arbitrary exported snapshots.

## Report meaning and limits

Retain plan bytes, canonical `EvaluationReport::encode()` JSON and content
digests as immutable artifacts. Encoding versions are independent of existing
wire schemas. No new endpoint, database migration or runtime activation occurs.

Each result includes profile and full trace hashes, transition count (excluding
genesis), order identities, filled shares and contracts separately, cumulative
execution/settlement fees and exact realized net USD cents. Net reconciles final
cash against initial cash PLUS initial inventory book cost. Fee-exclusive gross
cash flow sums absolute per-revision cash movement including settlement; it is
not normalized trade turnover. Peak-to-trough cash-plus-book-cost decline is
explicitly `book_drawdown_cents`, NOT market-to-market drawdown. Conservative
P06 risk marks are never used as fresh equity marks. Baseline differences remain
in declared case order; no best-case selection or monotonic-profit claim.

One path and its revisions are not independent statistical samples. Output says
`not_estimated_single_synthetic_path`; no confidence interval, Sharpe, annualized
return, calibration or regime claim is fabricated. Independent empirical paths,
qualified marks, chronological population coverage, sensitivity beyond fees /
virtual delays / participation (spread, queue/slippage), public storage/export,
performance and restore qualification remain M08/M10 follow-through.

## Verification

```powershell
cargo xtask verify --check frozen-evaluation-contracts
cargo xtask verify
```

Six named default proofs cover exact calculator outcomes, byte-identical repeated
and checkpoint-resumed reports, frozen census/identity, chronology/checkpoint
failures, partial/foreign/conflicting/open histories and payload-safe diagnostics.
The harness rejects removed, renamed, duplicated or unregistered proofs. Fixtures
are invented; no PostgreSQL, Docker, NATS, public cloud or paid services required.
