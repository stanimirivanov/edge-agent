# ADR-0034: Enforce versioned simulated-account risk policy

- Status: Accepted
- Date: 2026-10-20
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- P06 adds pure risk policy/state to the simulation-domain crate, without a service,
  database, provider, broker, model call or new dependency.
- Enforce account-wide caps and per-strategy caps over complete account snapshots,
  including reservations. Actual breaches latch; prospective rejection alone does not.
- Sessions are explicit calendar-bound intervals. Rollover never clears a latch.
- Policy-authorized, revision-checked recovery returns audit facts. P08 must enforce
  these decisions and retain state/audit atomically under the account lock.

## Context

Independent strategy budgets cannot control shared account exposure across replicas.
Restart, stale valuations and automatic daily resets must not erase breaches or
reservations. P06 supplies deterministic policy semantics for P08, not the database
serialization or order lifecycle that those later slices must prove.

## Decision

Use `simulation_domain::risk` values, immutable policies and single-owner risk state.
No generic policy framework or additional crate is needed. Risk money is exact
nonnegative USD cents, distinct from per-share prices, quotation points and ledgers.
Callers convert finer units conservatively under an independently pinned rounding
rule; this module never rounds, prices, nets currencies or calculates Greeks.

`RiskUsage::new` adds position exposure and outstanding reservations with checked
arithmetic. Pending cancellations remain reserved until committed terminal effect.
Session loss is each strategy's nonnegative adverse contribution from a qualified
session-opening baseline, including realized/unrealized loss and costs. Sum adverse
contributions; profitable strategies do not offset losing ones. This conservative
rule is explicit, not net account P&L or a general drawdown/margin model.

Policies pin a semantic revision, positive valuation age, inclusive gross/loss/order
caps, 1..32 distinct strategies and 1..16 authorized reset operators. Zero caps are
valid; there are no guessed financial defaults. Require exactly one valuation row
for every configured strategy, including inactive/zero allocations. Account caps
apply before strategy caps; one strategy cannot override shared limits.

The policy fingerprint is SHA-256 of `EA-RISK1/SUM-ADVERSE/USD-CENTS\0`, revision ID
(u128), revision (u64), freshness nanoseconds (u128), three account ceilings (u64
each), strategy count (u64), sorted strategy ID (u128) plus three ceilings per row,
operator count (u64), sorted operator IDs (u128). All integers are big-endian.
Bind this fingerprint and qualified valuation/baseline/rounding artifacts in P05's
run configuration. A declared revision alone cannot prove immutable policy content.

Risk state starts at revision 1 for a NEW account and freezes its policy. Observe/
increase checks require the expected risk revision and nonregressing injected time.
Validate account/session, complete strategies and future timestamps before decisions.
Outside the session denies; over-age valuation denies as stale. Mark freshness
is independent of the current loss baseline: a qualified prior-session mark may
be within the explicit age limit, without refreshing its original timestamp.
Account gross/loss/orders, then sorted strategy gross/loss/orders, determine the
first limit reason. Equality passes. Overflow rejects without partial mutation.

Actual current breaches latch the original reason. Better valuations do not heal
it. A projected increase can be denied without latching a currently healthy account.
Successful assessments, including denials, advance risk revision; errors do not.
These decisions gate risk INCREASES, not a future cancellation/emergency-close API.

Reset requires a policy-authorized actor, current revision, existing latch and fresh
healthy current-session snapshot. Rollover requires an authorized actor, increasing
date, non-overlapping interval and time inside the new session. Rollover preserves
the latch and does not mutate holdings/reservations or calculate a new loss baseline.
Both return actor/revision/policy-fingerprint/time/session audit facts for retention.

## Alternatives considered

Independent budgets, profit-netted loss and automatic session reset can conceal
aggregate exposure or clear a breach without recovery evidence. Reject for this
initial conservative profile; different loss semantics require a new version.
Putting SQL/authentication in policy would invert domain ownership. Defer those
adapters and their real concurrency proofs to P08 rather than claim them here.

## Consequences

Offline deterministic tests establish policy semantics, not snapshot authority,
funding, completeness of valuations, market qualification or distributed admission.
P07/P08 must consume the policy; P09 supplies qualified exposure/valuation arithmetic.
This does not complete M05 or implement all strategy/artifact constraints.

## Compatibility and migration

Additive Rust API only; no wire/schema/database change. P07/P08 introduce product
contracts through existing registry compatibility gates. Never reinitialize an
existing account with an empty latch after crash or policy change. P08 must retain
or reconstruct the complete risk history and carry breaches through explicit policy
evolution. Reverting this unused domain addition needs no deployed migration.

## Security and operations

Operator IDs are not credentials: the adapter must map a verified authenticated
principal, not trust a caller-supplied ID. State revision is not distributed fencing.
P08 must lock the authoritative account root, read all strategies/reservations and
qualified valuations, apply risk plus order/reservation changes, and commit state,
recovery audit, inbox and outbox atomically. Projections and concurrent pure checks
cannot authorize admission. Authentication, storage/restore, stale-owner fencing and
all funding/risk checks remain mandatory before runtime activation.

## Validation

Run `cargo xtask verify`. Focused tests cover aggregate/per-strategy boundaries,
reserved exposure, missing/duplicate strategies, future/stale/nanosecond valuations,
arithmetic/revision overflow, sticky breaches, authenticated-principal policy checks,
rejected resets, forward sessions and carried exposure. Independent SHA-256 vectors
protect policy meaning; compile-fail tests protect ID namespaces. No service-backed
admission, funding or provider qualification is claimed. See the
[risk guide](../development/simulation-risk.md).
