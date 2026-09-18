# EdgeAgent product vision

## TL;DR

- EdgeAgent turns a bounded market-research request into an auditable, time-limited research artifact.
- The first supported domain is liquid U.S.-listed equities and ETFs over approximately 3–20 trading days.
- Deterministic services own market facts, features, rankings, simulations, and risk; generated language explains validated evidence.
- Every artifact exposes sources, timestamps, assumptions, uncertainty, expiry, and invalidation conditions.
- Missing, stale, contradictory, or unlicensed critical evidence produces an abstention.
- Execution, custody, order routing, and guaranteed outcomes are outside the project boundary.

## Purpose

Market research is fragmented across price history, fundamentals, events, news,
technical calculations, and risk rules. The fragmentation makes analysis slow,
inconsistent, difficult to reproduce, and vulnerable to selective reasoning.
EdgeAgent provides a structured research workflow that converts an explicit
question into inspectable evidence and a bounded conclusion.

The project is useful when it improves the quality and reproducibility of a
research process. It is not successful merely because it produces persuasive
text or a large number of ideas.

## Primary outcome

A user or client submits a **Research Request** containing:

- security universe and exclusions;
- research horizon and effective time;
- one or more approved strategy families;
- hypothetical risk constraints;
- desired evidence and explanation depth; and
- optional benchmark or comparison set.

EdgeAgent either returns a versioned **Research Artifact** or an explicit
**Abstention**. Both results are useful. An abstention states which requirement
failed—for example stale prices, conflicting sources, insufficient history,
unsupported universe, missing entitlement, strategy ineligibility, or policy
rejection—without inventing continuity.

## Initial scope

The first complete vertical slice targets:

- liquid U.S.-listed common equities and broad or sector ETFs;
- long-only research scenarios;
- approximately 3–20 trading-day horizons;
- daily bars, with intraday observations added only where a strategy requires them;
- a small set of independently versioned strategies, beginning with momentum;
- deterministic volatility and risk illustrations;
- source-linked explanations; and
- paper outcome tracking under explicit fill and cost assumptions.

The initial user surface may be a CLI or API. A web or conversational adapter
must use the same request, policy, evidence, and artifact contracts rather than
reimplementing research logic.

## Research artifact contract

A published artifact contains at least:

- stable artifact, instrument, request, strategy, and policy identifiers;
- instrument venue and currency;
- direction and research horizon;
- event, observation, decision, and publication times;
- source and transformation lineage for every numeric or time-sensitive claim;
- evidence-backed thesis and counter-thesis;
- reference price and time;
- entry scenario, invalidation condition, target scenario, and expiry;
- deterministic liquidity, volatility, downside, and risk illustrations;
- known catalysts, scheduled event risk, gaps, and assumptions;
- component evidence scores and an overall calibrated score or abstention;
- lifecycle status; and
- the exact contract and implementation revisions required for replay.

The artifact is immutable once published. Subsequent observations append
lifecycle events such as `invalidated`, `expired`, `withdrawn`, or `corrected`.
A correction links to the original artifact and explains the cause; it does not
rewrite history.

## Core product invariants

### Facts are deterministic

Market prices, indicators, fundamentals, rankings, risk values, position-size
illustrations, simulations, and outcome metrics originate in deterministic
code. A language model may plan bounded retrieval or explain validated values,
but cannot create or modify authoritative facts.

### Evidence is point-in-time

Research uses only evidence available as of the request's effective time.
Source event time, observation time, ingestion time, and transformation version
remain visible. Corporate-action adjustments, market calendars, symbol identity,
and provider revisions are explicit.

### Policy is independent

Product eligibility, data-quality thresholds, strategy validity, and risk rules
execute outside the language model. The publication gate can approve, narrow,
redact, expire, or reject an artifact. Generated content cannot override it.

### Uncertainty is visible

EdgeAgent exposes data gaps, conflicting evidence, sensitivity, regime fit, and
counter-evidence. Confidence is a calibrated evidence measure with documented
components—not a stylistic assertion. The system abstains when evidence cannot
support the requested conclusion.

### Evaluation is honest

Backtests and paper outcomes use frozen strategy versions, point-in-time
universes, corporate actions, delistings, representative spread and slippage,
declared fills, and chronological holdouts. Results include sample size,
uncertainty, baseline comparison, regime breakdown, and drawdown. Win rate alone
does not establish value.

## Generalized platform capabilities

EdgeAgent is composed from reusable capabilities rather than channel-specific
features:

1. **Research request compiler** — converts validated input into one typed request.
2. **Market data fabric** — normalizes provider data, quality, freshness, identity, and lineage.
3. **Deterministic analytics runtime** — computes features and risk under versioned formulas.
4. **Strategy registry and runner** — executes only approved, reproducible strategy versions.
5. **Evidence graph** — links claims to raw and derived evidence.
6. **Policy and risk gate** — enforces publication and abstention rules.
7. **Explanation composer** — renders validated evidence without making new decisions.
8. **Evaluation and simulation ledger** — measures historical and live artifact outcomes honestly.
9. **Surveillance service** — appends expiry, invalidation, or withdrawal events.
10. **Audit ledger** — records material versions, decisions, and administrative actions.

Each capability exposes a small contract. A provider, model, storage engine, UI,
or workflow tool is an adapter and cannot redefine domain meaning.

## Non-goals

The current project does not:

- execute, route, stage, or transmit orders;
- connect to a brokerage account or hold assets or credentials for execution;
- guarantee profit or represent uncertainty as certainty;
- support options, leverage, short selling, micro-cap promotion, or copy trading;
- infer a person's income, tax status, liabilities, total assets, or suitability;
- continuously retrain production strategies from unreviewed user feedback;
- treat social sentiment or generated narrative as sufficient evidence; or
- depend on one proprietary provider or model for domain behavior.

Any proposal to cross an execution boundary requires explicit project approval,
a new threat model, an accepted ADR, and a separately defined milestone.

## Success measures

Early success is technical and research-oriented:

- 100% of published numeric claims resolve to validated evidence.
- At least 99.5% of evaluation artifacts pass deterministic numeric reconciliation.
- At least 95% of deliberately stale, contradictory, incomplete, or unlicensed
  critical-data cases produce an abstention.
- At least 99% of retained artifacts reproduce their deterministic fields from
  pinned inputs and versions.
- Each production strategy clears a declared walk-forward baseline net of
  stated costs and remains inside its maximum-drawdown policy.
- Default tests run without private credentials, paid services, or mutable external data.

These are initial quality gates, not claims about investment returns. Thresholds
may evolve through versioned policy and recorded evidence rather than silent
relaxation.

## Near-term validation question

The first end-to-end milestone must answer one narrow question: can EdgeAgent
take a fixed, synthetic point-in-time dataset and a typed momentum research
request, then produce either a fully reproducible ranked artifact or a correctly
classified abstention without using a model or live provider? Establishing that
deterministic spine comes before conversational presentation, additional
strategies, live data, or persistence.
