# Simulated account risk policy

## TL;DR

- P06 implements synchronous risk policy in `crates/simulation-domain/src/risk`.
- Include all strategies, positions, reservations and fresh qualified valuations.
- Actual breaches latch; projected over-limit orders are denied without latching
  healthy accounts. Rollover does not clear breaches or release exposure.
- P08 must enforce these rules under the authoritative account transaction. A
  `Permitted` result is not a reservation, funding check or durable admission token.

## Values and decisions

Construct a versioned `RiskPolicy` with explicit inclusive account ceilings and
every strategy's ceilings: gross exposure, adverse session loss and open orders.
Use exact `UsdCents`, never price/point values or floating-point cash. Conversion
from finer units must be conservative and separately qualified/pinned.
`RiskUsage::new` adds position and reserved exposure; pending cancellations remain
included until terminal effect commits. No shorting, leverage or margin engine.

Each `RiskSnapshot` contains exactly one row per configured strategy, including
zero/inactive allocations. Rows are sorted by stable ID; duplicates, omissions,
unknown allocations and wrong account/session fail closed. The caller must read
the complete authoritative account, not let a strategy select which rows to send.
Loss sums nonnegative adverse strategy contributions, including costs and marked
loss from the session baseline. Gains do not offset losses elsewhere. P09 must
qualify valuation inputs and calculation; P06 consumes those claims, not prices.

Freshness accepts equality at the pinned maximum age. Future valuations reject;
over-age valuations deny. A qualified prior-session mark may be within that age
budget; it is not the new session's loss baseline. Receipt/replay of an old quote
cannot refresh its valuation time. Missing valuations never become zero exposures.
Half-open session bounds use explicit qualified date/interval/calendar revisions,
not midnight guesses or fixed 24-hour exchange calendars.

`RiskState::observe` assesses actual current usage; `check_increase` also checks a
prospective increase. Both require the expected risk revision and injected monotonic
time. Validation precedes mutation. Current breach reason stays latched; a rejected
proposal alone does not latch. Errors never mutate; successful checks, including
denials, advance the revision. Persist decisions/revisions with effects in P08.
Reason precedence: session, stale valuation, account gross/loss/orders, then sorted
strategy gross/loss/orders. An existing latch retains its original reason.

## Recovery and trust

Only a policy-authorized operator may reset a latched breach after a fresh healthy
assessment. The application maps an authenticated principal; accepting an arbitrary
operator ID from HTTP/LLM/user input is NOT authentication. Old revisions, unhealthy
evidence and unauthorized resets leave state unchanged.

Authorized rollover requires a forward non-overlapping explicit session currently
containing decision time. It preserves the latch and changes no positions/orders/
reservations. P08 must carry them and establish the new session-loss baseline in
the same account transaction. A new date never grants a fresh exposure budget.
Reset/rollover return audit records including actor, revision, policy fingerprint,
time and resulting session; retain them with state/outbox atomically.

Freeze policy fingerprints, valuation/baseline/rounding/calendar artifacts in P05's
configuration. Never create a new empty `RiskState` to recover an existing account,
clear a breach or replace a policy. Persistence/history reconstruction, account
fencing, authorization adapters, funded admission and message schemas belong to
P07/P08 and remain unimplemented. Pure checks can race if used outside that boundary.
Breaches govern increases; future cancellation/close controls need explicit policy.

## Verification

Run `cargo test --locked -p edgeagent-simulation-domain` and `cargo xtask verify`.
Workspace CI discovers risk unit/integration/doctests without new tools or services.
Tests cover every implemented ceiling, reservations, exact boundaries, input/order
independence, missing rows, stale/future evidence, latching, reset/rollover authority,
revision/financial overflow, redacted diagnostics and fingerprint vectors.
See [ADR-0034](../decisions/0034-enforce-versioned-simulated-account-risk-policy.md).
