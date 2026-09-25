# EdgeAgent product vision

## TL;DR

- EdgeAgent turns a bounded market-research request into an auditable, time-limited research artifact and can execute an approved scenario through a dry-run order simulator.
- A Rust monorepo produces multiple independently deployable, event-driven services that run on portable Kubernetes or managed GCP, Azure, and AWS profiles.
- The first supported domain is liquid U.S.-listed equities and ETFs over approximately 3–20 trading days.
- Deterministic services own market facts, features, rankings, policy, simulated fills, positions, and P&L; generated language only explains validated evidence.
- Missing, stale, contradictory, or unlicensed critical evidence produces an abstention, and unsupported execution modes fail closed.
- Live brokerage connectivity, custody, and real-money order transmission remain outside the project boundary.

## Purpose and strategic value

Market research is fragmented across price history, fundamentals, events, news,
technical calculations, risk rules, and hypothetical execution assumptions. That
fragmentation makes analysis slow, inconsistent, hard to reproduce, and prone to
selective reasoning. EdgeAgent turns an explicit question into inspectable
evidence, a bounded conclusion, and—when requested—a reproducible dry-run trade
lifecycle.

The project also demonstrates production-oriented Rust architecture: explicit
service ownership, durable asynchronous workflows, immutable artifacts, portable
deployment contracts, and failure recovery under duplicate or delayed delivery.
This architecture creates reusable platform primitives rather than coupling the
system to a single interface, provider, cloud, or strategy.

## Primary outcomes

A user or client submits a **Research Request** containing a security universe,
horizon, effective time, approved strategy families, hypothetical risk
constraints, and required explanation depth. EdgeAgent returns a versioned
**Research Artifact** or an explicit **Abstention**.

For an approved artifact or an explicitly labeled test scenario, a caller may
submit a **Dry-Run Order**. EdgeAgent validates the order, simulates its lifecycle
against point-in-time observations and pinned cost assumptions, and exposes
immutable order, fill, position, cash, and P&L records. No dry-run operation can
reach a live venue or brokerage account.

An abstention and a rejected dry-run order are useful outcomes. Both expose
stable reason codes—for example stale prices, conflicting sources, insufficient
history, unsupported universe, missing entitlement, policy rejection, closed
market, invalid quantity, exceeded limits, or an execution mode other than
`dry_run`—without inventing continuity.

## Initial scope

The first complete vertical slice targets:

- liquid U.S.-listed common equities and broad or sector ETFs;
- long-only research and dry-run execution scenarios;
- approximately 3–20 trading-day research horizons;
- daily bars, with intraday observations added only where fill or strategy policy requires them;
- a small set of independently versioned strategies, beginning with momentum;
- market and limit dry-run orders with deterministic partial-fill behavior;
- explicit spread, slippage, fees, latency, liquidity, gaps, halts, and corporate-action assumptions;
- source-linked explanations and immutable artifact lifecycle events; and
- position, cash, realized P&L, and unrealized P&L projections.

The initial user surface may be a CLI or API. The separately owned web UI and
any conversational adapter must use the same request, policy, event, evidence,
order, and artifact contracts rather than reimplementing domain logic. They
communicate through the gateway and never directly access internal services or
data stores.

## Research artifact contract

A published artifact contains stable request, instrument, strategy, policy, and
artifact identifiers; market and decision time; currency and units; evidence
lineage; thesis and counter-thesis; horizon and expiry; reference price; entry,
target, and invalidation scenarios; deterministic risk illustrations; uncertainty;
and exact implementation revisions needed for replay.

The artifact is immutable once published. Subsequent observations append
`invalidated`, `expired`, `withdrawn`, or `corrected` events. A correction links
to the original and explains its cause; it never rewrites history.

## Dry-run execution contract

A dry-run order contains:

- stable order, request, artifact, account-simulation, and instrument identifiers;
- the literal execution mode `dry_run`;
- side, order type, quantity, optional limit price, and time-in-force;
- submission, effective, expiry, and observation times;
- risk-policy and simulation-policy versions;
- idempotency, correlation, and causation identifiers; and
- an explicit statement that no live order was transmitted.

Order state is append-only. Fills link to the market evidence and assumptions
that produced them. Ledger entries balance, duplicate delivery cannot duplicate
an order or fill, and replay from retained inputs must reproduce deterministic
state within declared numeric tolerances.

## Core product invariants

### Facts and simulated execution are deterministic

Market values, indicators, rankings, risk values, order validation, fills,
positions, cash, and P&L originate in deterministic Rust code. A language model
may plan bounded retrieval or explain validated values, but it cannot create or
modify authoritative facts, policy, or execution state.

### Evidence is point-in-time

Research and simulation use only evidence available at the declared effective
time. Source event time, observation time, ingestion time, transformation
version, corporate-action basis, calendar, symbol identity, and provider
revision remain visible.

### Policy is independent

Product eligibility, evidence quality, strategy validity, and research and
dry-run risk rules execute outside the language model. Generated content and
transport adapters cannot override policy decisions.

### Execution is dry-run by construction

The execution simulator has no live mode, broker credentials, broker endpoint,
or broker adapter. Runtime identity and network policy reinforce that code-level
boundary. Live execution would require a separate deployable and explicit
architecture, security, governance, and project approval.

### Events are recoverable facts

Cross-service delivery is at least once. Commands and events are versioned;
stateful producers use transactional outboxes; consumers use durable inboxes and
idempotent transitions. Ordering is scoped to declared aggregate keys, and
poison messages are quarantined for audited operator replay.

### Evaluation is honest

Backtests and dry-run outcomes use frozen strategy versions, point-in-time
universes, corporate actions, delistings, representative spread and slippage,
declared fills, and chronological holdouts. Results include sample size,
uncertainty, baseline comparison, regime breakdown, turnover, and drawdown.

## Generalized platform capabilities

1. **Request and command gateway** — validates identity, quotas, schemas, and durable command acceptance.
2. **Market data fabric** — normalizes provider data, quality, freshness, identity, rights, and lineage.
3. **Deterministic research runtime** — computes features and risk under versioned formulas.
4. **Strategy registry and runner** — executes only approved, reproducible strategy versions.
5. **Evidence graph** — links claims and fills to raw and derived evidence.
6. **Policy and risk gate** — controls publication, abstention, and dry-run eligibility.
7. **Explanation composer** — renders validated evidence without making new decisions.
8. **Dry-run execution engine** — models order state, fills, positions, cash, and P&L.
9. **Event backbone** — supplies durable commands, events, replay, quarantine, and projections.
10. **Audit and projection runtime** — records material transitions and builds disposable read models.
11. **Surveillance workers** — append expiry, invalidation, withdrawal, and reevaluation events.
12. **Deployment profiles** — promote identical signed images across portable and managed environments.
13. **Trusted interaction adapter** — presents evidence, abstentions, explanations, and dry-run state without becoming a source of domain truth.

Each capability exposes a small contract. A provider, model, storage engine,
message broker, UI, or cloud service is an adapter and cannot redefine domain
meaning.

## Non-goals

The current project does not:

- transmit live orders, connect to brokerage accounts, or hold broker credentials;
- provide custody, settlement, deposits, withdrawals, or portfolio accounting for real assets;
- guarantee profit or represent uncertainty as certainty;
- support options, leverage, short selling, micro-cap promotion, or copy trading;
- infer a person's income, tax status, liabilities, total assets, or suitability;
- continuously retrain production strategies from unreviewed user feedback;
- treat social sentiment or generated narrative as sufficient evidence;
- promise active-active cross-cloud operation or transparent zero-downtime provider failover; or
- depend on one proprietary provider, model, message broker, or cloud for domain behavior.

## Success measures

Early success is technical and research-oriented:

- 100% of published numeric claims and simulated fills resolve to validated evidence.
- 100% of execution requests with a mode other than `dry_run` are rejected before reaching the simulator.
- Zero broker credentials, broker endpoints, or live-order adapters exist in build artifacts or runtime configuration.
- At least 99.5% of evaluation artifacts pass deterministic numeric reconciliation.
- At least 95% of deliberately stale, contradictory, incomplete, or unlicensed critical-data cases produce an abstention.
- At least 99% of retained artifacts and dry-run ledgers reproduce from pinned inputs and versions within declared tolerance.
- Duplicate, delayed, and redelivered messages create zero duplicate artifacts, orders, fills, or ledger entries in the conformance suite.
- Every supported deployment profile passes the same identity, messaging, storage, telemetry, restart, rollback, and restore tests.
- Default tests run without private credentials, paid services, mutable external data, or public cloud access.

These are quality gates, not claims about investment returns. Thresholds evolve
through versioned policy and recorded evidence rather than silent relaxation.

## Near-term validation question

The first end-to-end milestone asks whether independently deployable Rust
services can consume a fixed, synthetic point-in-time dataset, publish a
reproducible momentum artifact, process one authorized dry-run order, produce
deterministic fills and ledger state, and rebuild projections after duplicate
delivery and restart—without a model, paid provider, public cloud, or live
broker. That slice establishes the architecture before additional strategies,
cloud profiles, or presentation layers expand the surface area.
