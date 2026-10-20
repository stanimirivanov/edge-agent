# ADR-0037: Commit evidence-driven simulation execution

- Status: Accepted
- Date: 2026-10-20
- Milestone: M07 - Dry-run execution
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- P09 uses consumed trades or the executable option quote side, never bars or
  theoretical prices. Every fee, participation and virtual delay is explicit.
- Reuse P08's account root and inbox transaction; also fence run-wide liquidity.
- Persist lifecycle, balanced accounting, reservations, continuations and outbox
  together. This qualifies synthetic correctness, not a live/provider deployment.

## Context

P08 prevents competing admission from spending the same account funds. Execution
must not recreate displayed liquidity on restart, multiply rounding across fills,
release pending cancellations early, or treat book cost as a fresh risk valuation.

## Decision

Use a finite, owner-authored execution plan plus a mandatory versioned profile.
The combined admission/execution schedule has unique contiguous account slots and
nonregressing virtual time. Requests reference the plan, slot and expected account
revision; they supply no actor, evidence, price, size, fee or time. Retain complete
canonical plan/profile artifacts and reject configuration changes on restart.

Underlying fills use positive-size trades only. Options use ask for buys, bid for
sells, the exact pinned cash/European/index specification and known positive side
size/premium. Floor participation to whole units. Reject ambiguous evidence, bars,
index proxies, unconsumed artifacts and corrected observations. This initial model
supports original observations with empty condition-code sets only; it never
retroactively changes a fill. Empty codes do not certify session eligibility.

Serialize each `(manifest, liquidity budget)` after the account root. Trades share
capacity across both sides; options have separate side budgets keyed by feed,
instrument and side timestamp, not a snapshot wrapper ID. Same-time changed content
conflicts, and refreshed wrappers cannot replenish volume. A genuinely later side
timestamp supplies a new hypothetical displayed budget under this versioned model;
that is not a claim about real replenishment or queue position. Different frozen
runs remain independent simulations. Conflicting profiles cannot share a budget.

Buy notional rounds up and sell notional down cumulatively at order level in USD
cents. Retain the exact checked fixed-scale subtotal. Fees use explicit fixed and
per-unit millionths-of-a-cent rates; charge the fixed component on first fill and
round the cumulative fee up. Independently enforce P08 price/fee holds. Maintain
long-only weighted-average total book cost, carrying partial-sale division residue
in the remaining lot. Cash, position cost, realized P/L expense and fee expense
postings must sum to zero. Initial book costs are explicit, never inferred.

Submission/cancellation delays use virtual time only. A pending cancellation stays
in the retained journal. Its confirmation cannot precede the configured due time;
equal-time canonical sequence determines the winner. After its due time a later
operation confirms it rather than filling; deadline expiry takes precedence.
No-fill attempts are retained too. Exact retries return before checking current
revisions; changed requests conflict. Root locking serializes all account writers.

Partial fills consume cash and inventory holds; only terminal facts release the
remainder. Conservative risk carry-forward never reduces gross on sale or refreshes
a valuation's timestamp/artifact. Purchases add gross at their funded acquisition
amount; realized losses and fees increase adverse contribution. P06 stale-mark and
breach latches remain authoritative. This is NOT a market-to-market refresh API;
subsequent admission can fail closed until separately qualified marks exist.

## Alternatives considered

- Per-order/per-process liquidity permits strategies and replicas to double spend.
- Intrabar inference, unknown sizes, queue/impact guesses or wall-clock sleeps
  manufacture execution meaning and make replay nondeterministic.
- Rounding each fill or including fees in cost hides split-fill and P/L errors.
- A new transaction, SQL driver or unchecked SQL obscures the commit boundary.
- Public service activation without authenticated composition and provider/session
  qualification grants authority that these synthetic proofs do not establish.

## Consequences

The finite plan is bounded to 4096 combined slots and 1 MiB of canonical execution
data. Order journals retain at most 4096 facts; no eviction hides duplicates.
Prices, sizes, times, book costs and configuration fail closed on overflow/missing
input. One account and one shared budget serialize deliberately. No stochastic
jitter, slippage beyond observed side/trade, depth, settlement or exercise is added.
P10 owns expiry settlement; P12/P13 own projections and fidelity qualification.

## Compatibility and migration

Admission v1 messages/schemas remain unchanged. Add execution v1 commands/facts and
an expand-only private migration. Keep admission receipts immutable; retain their
actual domain journal for new orders. Legacy orders without that journal cannot
execute: do not guess history or backfill financial values. Apply migrations with
the owner role, then explicitly provision profile/plan and book costs. Runtime
cannot change those artifacts/grants. Rollback pauses ingress and retains all
financial evidence; never drop tables or revert cash to downgrade.

## Security and operations

Bind verified principals outside messages; a source URI grants nothing. No external
I/O enters the transaction. READ COMMITTED and explicit finite transaction budgets
remain mandatory. Account root precedes every financial read and shared budget
lock; never acquire another account root while holding a liquidity lock. ACK only
after inbox commit. Financial diagnostics remain redacted. No live broker or model
provider/runtime exists. Ready evidence is not rights/calendar certification.

## Validation

Run `cargo xtask verify`, the SQLx freshness gate and the protected
`account-admission-processes` suite. Pure tests cover profile boundaries, exact
split-fill arithmetic, side selection, capacity and journal corruption. Independent
PostgreSQL workers qualify cash/inventory/options, shared strategies, rollback,
death before commit, lost acknowledgement, retained no-fill and pending cancel.
They do not certify provider rights, market impact, HA or power-loss durability.
