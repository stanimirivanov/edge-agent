# ADR-0003: Add dry-run trade execution

- Status: Proposed
- Date: 2026-09-18
- Milestone: M07 - Dry-run execution
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Add a dedicated execution-simulator service that accepts only dry-run order commands.
- Model order acceptance, rejection, cancellation, expiry, simulated fills, positions, cash, fees, and marked P&L as deterministic state transitions.
- Require a policy-approved, unexpired research artifact or an explicitly marked manual dry-run intent.
- Simulate venue calendars, latency, bid/ask spread, partial fills, slippage, gaps, halts, liquidity limits, and corporate actions.
- Persist an immutable order/event ledger and expose assumptions with every result.
- Do not include broker credentials, broker SDKs, order transmission, or a hidden live mode.

## Context

Research artifacts become more useful when their operational consequences can be
tested. A dry-run execution boundary demonstrates order lifecycle, asynchronous
processing, portfolio state, idempotency, market-event handling, audit, and risk
controls without placing a real order or connecting to a broker.

A simplistic “paper trade” that records a fill at the requested price would
produce misleading evidence. The simulator must state what it knows, encode
market mechanics explicitly, and distinguish a deterministic simulation from a
claim that the order would have filled in a live venue.

## Decision

EdgeAgent will add an independently deployable `execution-simulator` Rust
service. It consumes versioned dry-run order commands and market observations,
owns dry-run order/position/cash state, and publishes immutable lifecycle events.

### Supported initial behavior

The first version supports liquid U.S. equities and ETFs, long-only positions,
market and limit orders, day and good-till-time expiries, cancellations,
deterministic partial fills, and one base currency. Shorting, options, leverage,
margin, tax lots, multiple currencies,
and corporate brokerage accounts remain out of scope.

### Safety boundary

- The service accepts only the allowlisted `SubmitDryRunOrder` command contract;
  it exposes no generic execution command.
- Configuration has one execution mode: `dry_run`. There is no live enum value,
  broker endpoint, broker credential, or adapter trait in the initial design.
- Network egress is denied by default except for approved market-data and
  messaging endpoints required by the deployment profile.
- A future live-execution capability requires a new ADR, threat model, legal and
  security review, separate credentials, distinct command/event types, and an
  independently deployable service. It cannot be enabled through configuration alone.

### Command and event flow

1. `SubmitDryRunOrder.v1` identifies the idempotency key, account, instrument,
   side, order type, quantity, limit when applicable, time in force, effective
   time, originating artifact/manual intent, and policy revision.
2. The simulator validates schema, account state, instrument eligibility,
   artifact/manual-intent rules, market session, quantity and price units, and
   pre-trade risk.
3. It publishes exactly one local business outcome per accepted command identity:
   `DryRunOrderAccepted.v1`, `DryRunOrderRejected.v1`, or an exact retry response.
4. Accepted orders consume immutable market observations and transition through
   `open`, `partially_filled`, `filled`, `cancelled`, or `expired` states.
5. Fill, fee, cash, position, and realized/unrealized P&L effects commit atomically
   within the simulator's state store and publish through its outbox.
6. Portfolio and audit projectors consume lifecycle events idempotently.

### Simulation model

Every simulated fill records:

- market observation/evidence ID and event time;
- simulator and fill-model version;
- order arrival and eligible execution time;
- reference bid/ask or bar, depending on available data;
- spread, slippage, fee, and latency assumptions;
- requested and filled quantity, price, currency, and remaining quantity;
- liquidity cap and reason for partial or absent fill;
- market-session, halt, gap, and corporate-action context; and
- a quality classification reflecting the evidence resolution.

Daily-bar simulation cannot infer intrabar path. A limit touched by a daily bar
is not automatically filled unless the declared conservative fill rule permits
it. Intraday behavior requires corresponding point-in-time observations.

## Alternatives considered

### Record every idea as filled at its reference price

- Benefits: trivial implementation and easy performance charts.
- Costs and risks: systematic optimism, no order lifecycle, and false confidence.
- Reason not selected: the result would not demonstrate execution architecture or credible evaluation.

### Reuse the research service for dry-run execution

- Benefits: fewer deployables and direct access to artifacts.
- Costs and risks: couples research ranking to mutable portfolio/order state and
  obscures ownership, scaling, and security boundaries.
- Reason not selected: execution simulation has a distinct state machine and trust boundary.

### Integrate a broker paper-trading API

- Benefits: more venue-like behavior and less custom fill logic.
- Costs and risks: credentials, external mutable state, vendor coupling, rate
  limits, nondeterminism, and a short path from paper to live configuration.
- Reason not selected: the initial requirement is portable, deterministic dry-run execution.

## Consequences

### Positive

- The portfolio demonstrates orders, fills, positions, cash, risk, audit, and
  event-driven state transitions without financial exposure.
- Simulation assumptions are testable and versioned.
- Research artifacts can be evaluated through an operationally realistic boundary.
- The live-execution boundary remains visibly absent rather than hidden behind a flag.

### Negative

- Credible fill simulation needs market microstructure assumptions and more data.
- Portfolio state introduces concurrency, sequencing, reconciliation, and recovery requirements.
- Results still cannot prove that a live broker or venue would have produced the same fill.

### Neutral or follow-up

- A manual dry-run intent is allowed for testing but is labeled separately from
  a research-artifact-derived order.
- The account model is synthetic and stores no real brokerage identity.
- Execution quality can evolve through new fill-model versions without rewriting old outcomes.

## Compatibility and migration

Order and fill contracts are versioned and append-only. A fill-model change
applies only to new orders or an explicitly separate replay; it cannot mutate
historical fills. State rebuild consumes lifecycle events in aggregate sequence
order and verifies balances and position invariants.

## Security and operations

- Commands require authenticated caller identity and account-level authorization.
- Quantity, notional, open-order count, event age, and processing concurrency are bounded.
- Unknown instruments, stale market observations, closed sessions, invalid
  prices, unsupported order types, and policy failures reject or pause safely.
- The service exports reconciliation metrics for order state, positions, cash,
  event lag, duplicate commands, and projection drift.
- An operator kill switch rejects new orders while allowing reconciliation and read access.

## Validation

- Property tests cover order-state transitions, cash/position conservation,
  idempotency, cancellation races, partial fills, and event replay.
- Scenario fixtures cover market open/close, gaps, halts, splits, stale quotes,
  missing bid/ask, insufficient liquidity, duplicate commands, and delayed events.
- Failure-injection tests crash before/after commit and before message acknowledgment.
- A complete dry-run order can rebuild from its immutable event history with the same final state.
- Security tests prove that no supported command, configuration, or dependency can transmit a live order.
