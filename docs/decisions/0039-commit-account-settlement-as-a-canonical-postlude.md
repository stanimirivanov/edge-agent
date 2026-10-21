# ADR-0039: Commit account settlement as a canonical postlude

- Status: Accepted
- Date: 2026-10-21
- Milestone: M07 - Dry-run execution
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Append a retained, bounded synthetic settlement plan to finite P08/P09 work.
- Lock the account before authorization, receipts, positions and financial effects.
- Round account/instrument payoff once, then attribute deterministically to strategies.
- Commit order expiry, hold release, position closure, receipts and outbox together.
- Missing evidence changes no financial state; public/provider qualification remains separate.

## Context

P10a calculates payoff but cannot prevent duplicate credit or release pending
orders. Positions belong to strategies while cash and risk authority belong to
the account. Per-strategy rounding can manufacture money. Workers cannot settle
safely using projections or process-local completion flags.

## Decision

Reuse the PostgreSQL inbox-owned transaction. A separately retained settlement
postlude binds the execution plan, exact policy/specification and complete
synthetic fixing bytes, knowledge times, slots and explicit work limits. Provision
before admission. Slots follow the original finite schedule and all frozen market
inputs; no new scheduler or modification of P05 feed descriptors is introduced.
Full replay identity includes manifest, admission/execution configuration and
postlude. Reconstructed workers verify those retained artifacts.

New identity-only dry-run commands reference plan/slot/expected account revision.
Under READ COMMITTED, set explicit transaction-local budgets, lock the root,
check verified-principal grants and configuration hashes, then return exact
semantic receipts before checking new input/revision preconditions. Gaps retry;
changed requests fail. Source URIs alone confer no authorization.

Calculate using all strategy quantities and costs of one account/instrument.
Attribute the rounded gross and fee totals by cumulative quantity proportion in
ascending fixed-width strategy-ID byte order. Difference successive cumulative
floors; the last allocation consumes all residue. This is deterministic accounting
attribution, not another price model. Each strategy releases its actual book cost;
every attribution and aggregate receipt balances.

Known final evidence permits expiry of affected active orders and release of
their holds, then zeroing position/cost and charging separate fees. Unrelated
reservations still protect cash. Empty lots require final evidence before sealing
with zero effects. Missing/not-due/future-known evidence retains a deferred receipt
and advances canonical account/run/risk observation, without closing orders,
releasing holds or changing balances. Unfunded charges roll back the transaction.

Retain every input receipt and one terminal instrument seal. Later scheduled
commands produce already-settled zero-effect facts. Admission/execution receipts
remain immutable; active journals append expiry. Preserve P09 conservative risk
carry and sticky latches; do not fabricate fresh marks, resets or session rollover.

## Alternatives considered

- Separate commits expose partial expiry, financial closure or publication.
- Per-strategy rounding and row-arrival allocation change account cash.
- Ordinary marks cannot establish authoritative final settlement evidence.
- Process-local completion cannot survive death and competing workers.
- A general workflow scheduler exceeds this finite P10b boundary.

## Consequences

This completes synthetic finite P10 qualification, not provider finality, rights,
market fidelity, public activation, HA or all M07 work. American, physical,
discretionary exercise and correction reconciliation remain excluded. Explicit
order/allocation limits each lie in 1..=32; combined slots stay within 4096,
postlude bytes within 1 MiB and serialized receipts within 16 KiB. Exceeding a
bound rolls back. Delayed or funding-rejected settlement retains exposure and
requires explicit operator/run resolution, never a guessed zero payoff.

## Compatibility and migration

Add settlement v1 command/fact schemas without changing existing wire semantics.
Migration 0003 adds immutable configuration, balanced receipts and instrument seals.
No funding or old financial record is rewritten. Apply owner migrations, provision
costs/execution then the postlude, and grant only necessary runtime mutations.
Configuration remains SELECT-only; receipts/seals are append-only to runtime.
Legacy missing journals fail closed. Rollback pauses ingress and retains committed
cash, journals, receipts and seals; dropping tables is not a financial reversal.

## Security and operations

One fenced writable primary and one account-root lock serialize strategies and
replicas. No external call or broker ACK enters the transaction. Inbox commit
precedes ACK; cancelled futures and process death discard uncommitted effects.
Runtime cannot change policies, grants or funding. Diagnostics remain payload-safe.
No live broker, provider/model calls or deployable activation is added.

## Validation

Run protected account-admission-processes and simulation-schema-contracts gates,
SQLx freshness and default verification. Restricted independent workers exercise
ITM/ATM/OTM calls/puts, empty/fractional positions, pending expiry, missing/future
evidence, funding denial, duplicates/conflicts, authorization, gaps, competing
workers, artifact corruption and real worker death before/after commit. Lost
worker-result recovery does not claim a new broker-wire ACK fault, PostgreSQL
power-loss, HA or load proof.
