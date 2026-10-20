# Dry-run order lifecycle

## TL;DR

- `edgeagent_simulation_domain::orders` owns P07's pure lifecycle/proposal port.
- Exact `dry_run` is mandatory. No live variant, broker path or service activation.
- Retain contiguous immutable facts and complete duplicate evidence, including no-fill.
- P08 owns funded transactions/product messages, P09 qualified fills, P10 settlement.
  This foundation does not complete M07.

## Ownership and flow

1. Parse the required mode with `DryRunMode::parse`. Construct `OrderDefinition`
   with P05 run identity, P06 account/strategy IDs, deterministic order ID, typed
   instrument/quantity/price and explicit half-open lifetime.
2. P08 must authenticate ownership, enforce aggregate/strategy risk, reserve cash
   or long inventory and retain acceptance/rejection in the account transaction.
   Sell-to-close intent is not inventory authorization; a pure check is not a lock.
3. Reconstruct `Order` from its original submission and complete journal. Apply
   facts in canonical revision order; defer gaps for recovery. Compare full content
   on duplicates, including time, model/evidence and fill terms.
4. Consume P05's frozen inputs before simulation. `SimulationStep` binds ready
   evidence, one retained artifact digest per observation, an evidence-bundle digest
   and expected model revision/configuration. The order checks run/time, consumed
   provenance and matching product/specification evidence before model execution.
   A new readiness assessment may reuse identical consumed evidence later without
   changing frozen availability. An old assessment cannot authorize later work.
5. `Order::simulate` returns an uncommitted fill/no-fill fact. P08 must atomically
   retain the fact, order, reservations/ledger effects, inbox/outbox and run/model
   continuation. Discard uncommitted continuation after transaction failure.

The port permits pure synchronous computation only: no I/O, wall clock or hidden
entropy. P09 supplies qualified algorithms, latency, fees, liquidity and explicit
random scheduling. Retaining no-fill outcomes prevents resampling an existing
attempt; new IDs must not evade those budgets. Caller verification and qualified
rights/calendars remain necessary; artifact claims do not certify them.

## State and limits

Acceptance enables fills. Pending cancellation still permits fills and retains
reservations. Cancel rejection restores working/partial status; confirmation cancels
only the remainder. Full fill rejects later cancel; confirmed cancel rejects later
new fill. Canonical revisions break ties, not broker arrival time.

Execution stops at the exclusive deadline. Explicit expiry closes only the remainder,
preserves historical fills and does not settle the position. Exact duplicates remain
valid after terminal state or journal exhaustion; new incompatible facts fail. Choose
an event bound in 1..4096; exhaustion fails closed, without history compaction/eviction.

Only whole shares/contracts, buy-to-open/sell-to-close and market/limit styles exist.
Buy fills cannot exceed limits; sell fills cannot fall below them. Option fills must
fit the pinned tick. Specifications must be known at submission and cover the full
lifetime within effective/trading bounds. Cash/European/index structure is not
calendar, product, pricing or settlement qualification. Fractional shares and
amend/replace are intentionally unsupported.

## Verification and follow-through

Run `cargo test --locked -p edgeagent-simulation-domain`, then `cargo xtask verify`.
Tests use invented evidence and fake models, without a provider/runtime call.
P08 must add registry-checked product messages and prove multi-process admission,
rollback/restart/acknowledgement loss. P09 adds balanced ledger/reproducibility
qualification; P10 adds settlement. See [the plan](plans/simulation-expansion.md)
and [ADR-0035](../decisions/0035-apply-dry-run-order-lifecycles-from-retained-facts.md).
