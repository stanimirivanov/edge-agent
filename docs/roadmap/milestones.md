# EdgeAgent implementation milestones

## TL;DR

- Milestones are outcome-oriented and numbered M01 through M10 without renumbering after publication.
- The deterministic offline research spine comes before live providers, language models, web UI, or persistence.
- Each listed work item should fit one independently reviewable pull request.
- Market-data adapters must pass the same conformance contract and default CI remains offline.
- Generated explanation is introduced only after evidence, strategy, policy, and artifact validation are authoritative.
- Execution, custody, and broker integration are outside this roadmap.

## Milestone index

| Milestone title | Outcome |
|---|---|
| M01 - Engineering foundation | Make every change repeatable, reviewable, and safe. |
| M02 - Deterministic research spine | Produce the first reproducible artifact or abstention from synthetic evidence. |
| M03 - Market data fabric | Acquire and normalize point-in-time evidence without coupling domain policy to a provider. |
| M04 - Strategy registry | Run and evaluate approved strategy versions under stable contracts. |
| M05 - Policy-bound publication | Publish only artifacts that pass explicit evidence, product, and risk policy. |
| M06 - Evidence-grounded explanation | Explain approved research without allowing a model to create facts or decisions. |
| M07 - Durable artifact lifecycle | Persist, replay, invalidate, expire, withdraw, and correct research artifacts. |
| M08 - Evaluation and simulation | Measure strategy and artifact outcomes without hindsight or unrealistic fills. |
| M09 - Surveillance and delivery | Reevaluate named conditions and deliver bounded, deduplicated lifecycle updates. |
| M10 - Operational readiness | Run EdgeAgent securely, observably, recoverably, and at controlled scale. |

## M01 - Engineering foundation

**Message:** Make every change repeatable, reviewable, and safe.

Work items:

- Establish the EdgeAgent contributor and engineering harness.
- Add open-source governance, security reporting, issue forms, and pull-request guidance.
- Define the product vision, system architecture, domain integrity rules, and roadmap.
- Add a dependency-free repository verifier with tests.
- Run foundation verification on Linux and Windows in GitHub Actions.
- Record the initial runtime and packaging decision in an ADR.
- Scaffold the selected runtime with pinned formatting, linting, type checking,
  tests, packaging validation, and vulnerability checks.
- Add a one-command developer quickstart that matches CI.
- Establish dependency update and license-review policy in checked-in tooling.

Completion means a new contributor can understand the intended system, make a
reviewable change, and run authoritative repository and runtime checks without
private credentials.

## M02 - Deterministic research spine

**Message:** Produce the first reproducible artifact or abstention from synthetic evidence.

Work items:

- Define stable request, instrument, evidence, strategy, policy, and artifact identifiers.
- Define explicit market time, observation time, decision time, currency, price, return, risk, and freshness values.
- Define the versioned Research Request, Evidence Snapshot, Research Artifact, and Abstention contracts.
- Add redistribution-safe synthetic daily-bar fixtures covering normal sessions,
  missing bars, a holiday, a split, stale data, and conflicting evidence.
- Implement canonical bar validation and typed quality outcomes.
- Implement one versioned momentum calculation with declared lookback and adjustment basis.
- Implement deterministic candidate eligibility, ranking, and tie-breaking.
- Implement minimum evidence policy and abstention reason codes.
- Render a deterministic artifact without a language model.
- Add replay tests proving identical deterministic fields for identical versions and evidence.
- Expose the slice through a small local CLI after the domain flow is complete.

Completion means an offline request produces a reproducible ranked artifact or
a correctly classified abstention with no network, model, database, or UI.

## M03 - Market data fabric

**Message:** Acquire and normalize point-in-time evidence without coupling domain policy to a provider.

Work items:

- Define the market-data adapter contract and conformance fixture suite.
- Define stable instrument resolution independent of ticker strings.
- Implement one production-candidate historical daily-bar adapter behind the contract.
- Capture provider event time, observation time, adjustment basis, revision, entitlement, and provenance.
- Add bounded timeouts, cancellation, retries, rate-limit handling, and error classification.
- Implement freshness, gap, OHLC, volume, calendar, duplicate, and outlier validation.
- Add corporate-action and symbol-change reconciliation for the supported universe.
- Add a content-addressed local development cache whose keys include semantic versions.
- Define cross-source disagreement semantics before adding a second provider adapter.
- Gate live-provider tests outside default CI and document credential-safe setup.

Completion means the same domain request can use synthetic evidence or one live
provider through identical canonical contracts and failure semantics.

## M04 - Strategy registry

**Message:** Run and evaluate approved strategy versions under stable contracts.

Work items:

- Define strategy metadata, lifecycle, compatibility, and validation status.
- Replace the first hard-coded strategy with a registered version without changing results.
- Define feature requirements, eligibility, score, tie-break, horizon, expiry,
  benchmark, regime assumptions, and invalidation rules.
- Add a second strategy only after the registry demonstrates a real variation point.
- Separate experimental and approved strategy execution.
- Add deterministic configuration loading with strict unknown-field rejection.
- Record multiple-testing and promotion evidence for candidate strategy versions.
- Expose selection and omission reasons in the artifact.

Completion means strategies evolve independently under explicit versions while
the request, evidence, policy, and artifact contracts remain stable.

## M05 - Policy-bound publication

**Message:** Publish only artifacts that pass explicit evidence, product, and risk policy.

Work items:

- Define versioned policy input, decision, and reason-code contracts.
- Enforce supported instruments, directions, horizons, and strategy statuses.
- Add liquidity, volatility, price-gap, data-quality, and scheduled-event policy.
- Add hypothetical per-position and aggregate-heat constraints without inferring personal suitability.
- Define `approved`, `narrowed`, `abstained`, `rejected`, and `withdrawn` semantics.
- Add adversarial cases showing that no adapter or model can override policy.
- Implement artifact schema validation and exact numeric reconciliation.
- Add global and strategy-specific publication kill switches with audit events.

Completion means no artifact can be published without a reproducible policy
decision and complete numeric/evidence validation.

## M06 - Evidence-grounded explanation

**Message:** Explain approved research without allowing a model to create facts or decisions.

Work items:

- Define the explanation proposal schema and evidence-citation contract.
- Build an evidence allowlist containing only policy-approved facts.
- Add prompt and tool contracts that treat retrieved content as untrusted data.
- Implement one optional model adapter behind a model-neutral port.
- Parse unknown output strictly and reject uncited, altered, or invented values.
- Add prompt-injection, fabricated citation, conflicting instruction, and numeric mutation tests.
- Implement deterministic rendering as fallback for outage, timeout, budget, or validation failure.
- Pin model, settings, prompt, tools, and validator versions for replay.
- Evaluate groundedness and explanation usefulness on a frozen suite.

Completion means a model can improve presentation but cannot change the artifact's facts, ranking, policy, or lifecycle.

## M07 - Durable artifact lifecycle

**Message:** Persist, replay, invalidate, expire, withdraw, and correct research artifacts.

Work items:

- Record the database, migration runner, and evidence-storage decisions in ADRs.
- Add a disposable local persistence environment and complete migration-chain verification.
- Persist requests, immutable artifact publications, lifecycle events, and version references.
- Store evidence snapshots according to source rights and integrity requirements.
- Enforce append-only lifecycle and cross-scope integrity at application and database boundaries.
- Implement idempotent publication using identity plus immutable-content comparison.
- Rebuild an artifact's deterministic fields from retained evidence and pinned versions.
- Add retention, export, deletion, backup, and restore behavior for the supported deployment mode.
- Expose deterministic artifact retrieval and lifecycle history.

Completion means an artifact survives process restarts and remains replayable,
auditable, and historically honest.

## M08 - Evaluation and simulation

**Message:** Measure strategy and artifact outcomes without hindsight or unrealistic fills.

Work items:

- Define point-in-time dataset and evaluation-run manifests.
- Add chronological train/development/holdout partitions and leakage tests.
- Include delistings, corporate actions, market calendars, and universe history.
- Define fill timing, spread, slippage, fee, liquidity, gap, and halt behavior.
- Add simple declared baselines and comparison semantics.
- Report sample size, uncertainty, calibration, drawdown, turnover, and regime breakdown.
- Record experiment identity and multiple-testing history.
- Append paper outcomes without modifying original artifacts.
- Add sensitivity analysis for costs and fill assumptions.
- Prevent evaluation output from becoming a public claim without separate review.

Completion means approved strategy evidence and live artifact outcomes are
reproducible, comparable, and explicit about uncertainty.

## M09 - Surveillance and delivery

**Message:** Reevaluate named conditions and deliver bounded, deduplicated lifecycle updates.

Work items:

- Define surveillance subscriptions and lifecycle-trigger contracts.
- Evaluate expiry, invalidation, scheduled event, data-quality, and withdrawal conditions.
- Make reevaluation idempotent under duplicate and out-of-order evidence.
- Add bounded scheduling, concurrency, retry, dead-letter, and operator recovery semantics.
- Deduplicate notifications by artifact, lifecycle transition, and destination.
- Add one delivery adapter after lifecycle behavior is complete.
- Expose health and lag for evidence acquisition, reevaluation, and delivery.
- Demonstrate that surveillance cannot create an order or mutate an artifact.

Completion means published artifacts receive reliable, auditable state updates
without noisy duplicates or execution capability.

## M10 - Operational readiness

**Message:** Run EdgeAgent securely, observably, recoverably, and at controlled scale.

Work items:

- Define deployment identity, authentication, authorization, and tenant/scope boundaries.
- Add structured logs, traces, bounded-cardinality metrics, and decision audit views.
- Enforce request, symbol, lookback, provider, model, concurrency, queue, and storage budgets.
- Add health probes and explicit normal, degraded, and fail-closed state reporting.
- Package immutable deployment configuration and migrations.
- Add staged rollout, compatibility checks, rollback, and artifact-withdrawal runbooks.
- Verify backup, restore, retention, deletion, and disaster recovery.
- Define and alert on availability, latency, freshness, replay, validation, and abstention SLOs.
- Complete threat modeling, dependency/license review, and a production-readiness assessment.

Completion means operators can deploy, observe, limit, recover, and stop the
system without depending on a model or editing historical research.

## Planning rules

- GitHub owns live issue state, assignee, labels, and milestone assignment.
- This document owns intended sequencing until an issue is created.
- Every issue names one exact milestone and one observable outcome.
- Move an issue only when its outcome dependency changes; update this roadmap in the same planning change.
- Split an item when discovery reveals multiple independently valuable or risky capabilities.
- A milestone may defer work only when its outcome statement remains true and the deferral is explicit.
- Later milestones may begin discovery early, but implementation cannot bypass an unmet integrity dependency.
