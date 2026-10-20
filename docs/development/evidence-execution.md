# Evidence-driven dry-run execution

## TL;DR

- Mandatory versioned fees, participation and virtual delays; no defaults.
- P04 ready evidence must match P05's consumed frozen prefix.
- Reuse the P08 account transaction and lock a run-wide displayed budget.
- Commit balanced postings, lifecycle, holds, risk/run continuations and outbox.
- Synthetic qualification only; no public/provider/live execution activation.

Read [ADR-0037](../decisions/0037-commit-evidence-driven-simulation-execution.md)
and [account admission](account-admission.md) before modifying this path.

## Composition and configuration

The domain `ExecutionProfile::new` requires a semantic revision, participation in
1..10000 basis points, fixed/per-whole-unit fees in millionths of a USD cent, and
submission/cancellation `Duration`s. Supply zero explicitly. Latencies above one
day are unsupported, not truncated. These are structural limits, not suggested
production settings. Profile bytes/digest and plan bytes/digest are retained.

`AdmissionContext::new_scheduled` combines quote and execution slots into one
bounded, contiguous sequence. `ExecutionContext::new` binds each execution slot
to its original quote, action, virtual instant and ready artifacts. Provision
once with `provision_execution`, explicitly supplying quantity and total book
cost for every existing/targeted position. Unknown cost does not become zero.
Reconstruct the same verified context from retained run/catalog/evidence after
restart; changed bytes/configuration fail closed. The fixture values in
`crates/event-spine-conformance/src/accounts/execution.rs` are invented examples,
not production defaults or provider qualification.

The adapter implements the existing inbox transaction callback. Verified principal
binding and finite connection budgets belong to composition, not payload claims.
Apply owner migrations before composition; runtime may mutate only execution
state, liquidity, holdings, carry-forward risk and account continuations. Runtime
must not modify grants, quote/plan/profile artifacts, seed funding or book costs.

## Execution and accounting

Trades provide whole-share capacity; options provide positive known ask size for
buys or bid size for sells. Capacity floors participation; bars, corrected data,
index levels, unknown sizes and unconsumed evidence cannot fabricate a fill.
Nonempty condition-code sets are unsupported until independently qualified; empty
codes do not certify rights or a regular session. Limit protection and the buy
reservation ceiling are independent checks. Shared
budgets span strategies, replicas and accounts in the same run. Repeated snapshots
with the same side timestamp share capacity; inconsistent content conflicts.
Later side timestamps are explicitly new hypothetical snapshot budgets, not an
estimate of real replenishment. No queue/depth or intrabar inference is performed.

Exact fixed-scale order totals survive restart. Buys ceil and sells floor the
cumulative notional to cents; fees ceil the cumulative fee, once per order. The
fixed fee starts at first fill. Book cost uses weighted-average total cost and
retains partial-sale division residues. Fees are expenses, not position basis.
Each receipt balances cash + position cost + realized P/L expense + fees = zero.

Partial fills consume holds; definitive fill/cancel/expiry releases the remainder.
Pending cancel cannot release funds. Virtual submission/cancel delays and all
no-fill decisions are retained; equal-time canonical sequence resolves races.
After cancellation due time, a later step confirms cancellation; an expired
deadline wins. Expiry closes the order, not the option position (P10).

Risk carry-forward is deliberately conservative: no reduction of gross on sale,
no freshness/artifact rewrite, and separately accumulated fees/realized loss.
Stale valuations can latch a denial and block later admission. This is not a
market-to-market valuation refresh or risk reset API. P06 authorization/latches
remain unchanged.

## Recovery, migration and limits

Root `FOR UPDATE` precedes authorization, receipts, financial reads and the shared
liquidity lock. Check exact receipts before new revision checks. Conflicting
requests fail permanently; missing predecessors retry. All state and outbox
effects remain inside the inbox transaction; only commit permits ACK. Cancelled
futures/process death discard uncommitted financial and liquidity work.

The combined catalog is limited to 4096 slots, execution plan/journal bytes to
1 MiB, and each journal to 4096 facts. No duplicate history is evicted. Catalogs
are finite, not an unbounded continuously running day-trading engine. P10-P13
remain separate. M07 and provider/intraday fidelity are not declared complete.

Migration 0002 expands P08 without rewriting v1 wire schemas or old receipts.
Old orders lacking their actual admission journal fail execution; do not invent
history or infer book cost. Roll back by pausing composition while retaining all
financial tables/checkpoints; never reverse committed cash by dropping schema.

## Verification

```powershell
cargo xtask verify
cargo xtask verify --check account-admission-processes
cargo xtask verify --check sqlx-metadata
```

The latter gates use the existing explicitly authorized disposable local
PostgreSQL profile; see [SQLx metadata](sqlx-metadata.md). The protected process
suite now covers seven P08/P09 proofs, including real killed workers and lost ACK.
Default verification includes domain/wire tests but does not claim ignored
database proofs. No provider, broker, model or local LLM runtime is contacted.
