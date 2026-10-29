# EdgeAgent implementation milestones

## TL;DR

- Build one Cargo workspace that produces independently deployable Rust services, workers, migrations, and contract-test tooling.
- Establish versioned contracts and durable at-least-once event handling before adding live data or complex workflows.
- Prove research, policy, and dry-run execution with synthetic evidence before integrating paid services.
- Treat the portable Kubernetes profile as the reference and qualify managed GCP, Azure, and AWS profiles through the same conformance suite.
- Deliver the user experience as a separately governed gateway client after its backend contracts are proven.
- Keep default CI offline, deterministic, credential-free, and runnable by an open-source contributor.
- Live order transmission, custody, and broker integration remain outside this roadmap.

## Milestone index

| Milestone | Outcome |
| --- | --- |
| M01 - Rust engineering foundation | Build, test, package, and govern the workspace reproducibly. |
| M02 - Contracts and event spine | Prove recoverable cross-service delivery under duplicates and failure. |
| M03 - Market data fabric | Publish trustworthy point-in-time evidence through provider-neutral contracts. |
| M04 - Deterministic research | Publish the first reproducible artifact or abstention. |
| M05 - Strategy and policy | Evolve approved strategies without weakening independent controls. |
| M06 - Evidence-grounded explanation | Explain approved research without allowing generated content to create facts. |
| M07 - Dry-run execution | Process simulated orders, fills, positions, cash, and P&L without a live path. |
| M08 - Durable projections and evaluation | Rebuild lifecycle views and measure outcomes without hindsight. |
| M09 - Multi-cloud deployment profiles | Promote identical signed artifacts through portable and managed profiles. |
| M10 - Operational readiness | Operate, recover, limit, and stop the distributed system safely. |
| M11 - Trusted user experience | Expose research and dry-run workflows without moving domain authority into the browser. |

## M01 - Rust engineering foundation

**Outcome:** Build, test, package, and govern the workspace reproducibly.

Work items:

- Accept the Rust workspace, event backbone, and dry-run execution ADRs.
- Pin the stable Rust toolchain and Rust 2024 edition.
- Scaffold shared domain, application, contracts, infrastructure, testing, service, and worker crates.
- Enforce `rustfmt`, strict Clippy, documentation, tests, dependency policy, license policy, and vulnerability scanning.
- Produce minimal multi-stage, non-root OCI images for every deployable.
- Add a local dependency profile for PostgreSQL, NATS JetStream, S3-compatible storage, and OpenTelemetry collection.
- Generate an SBOM and provenance metadata and sign release images.
- Retain the repository verifier until Rust tooling covers each equivalent check.
- Run authoritative checks on Linux with one documented command.

Completion means a new contributor can build, test, inspect, and package every
deployable without private credentials, paid services, or undocumented setup.

## M02 - Contracts and event spine

**Outcome:** Prove recoverable cross-service delivery under duplicates and failure.

Work items:

- Define CloudEvents-compatible command and event envelopes with semantic schema versions.
- Define stable request, message, correlation, causation, aggregate, and idempotency identifiers.
- Establish subject naming, command ownership, partition keys, retention, and compatibility policy.
- Implement a NATS JetStream messaging adapter behind an application port.
- Implement transactional outbox relay and inbox deduplication with PostgreSQL.
- Add bounded retry, acknowledgement, backpressure, quarantine, and operator replay mechanics.
- Generate schemas and compatibility fixtures from contract crates.
- Test duplicate, delayed, reordered, malformed, poison, and acknowledgement-loss cases.
- Add traces and metrics across publish, delivery, handling, persistence, and acknowledgement.

Completion means a durable command can survive process and broker restarts and
produce one domain transition despite duplicate or delayed delivery.

The [M02 completion plan](../development/plans/m02-completion.md) sequences
remaining schema, topology, multi-process, admission, replay, telemetry, and
fault-qualification slices after the completed crate refactors. Its first slice
defines conformance-only typed acceptance contracts and the protected local
schema/compatibility gate; product APIs and worker-process evidence are not
claimed by that slice.

The second slice adds explicit registry-derived JetStream provisioning and
runtime-permission contracts with isolated authenticated broker proofs. See the
[topology guide](../development/jetstream-topology.md); no application worker,
transaction, production deployment, or completed M02 claim follows from it.

The third slice adds finite independent producer, relay and consumer processes
with private database identities and protected atomic acceptance, fact,
duplicate and rollback proofs. See [process acceptance](../development/process-acceptance.md).
This is tooling composition, not production worker readiness or full crash and
wire acknowledgement-loss qualification; later M02 slices remain required.

The seventh slice adds the [owned failure-qualification command](../development/m02-qualification.md)
and retained scenario evidence for actual worker kills, same-store broker
restart, directional wire ACK loss, post-deduplication duplicates and invalid
input. These synthetic single-node proofs do not qualify PostgreSQL server
restart, cloud/HA/load or production operations.

**PostgreSQL SQL verification slice:** Outbox enqueue and all inbox adapter SQL
use `sqlx::query!` or `sqlx::query_file!` with checked-in offline `.sqlx`
metadata generated from both adapters' migrations. Database-backed CI
regenerates and checks the metadata; default builds remain database-free.
Convert remaining outbox adapter SQL in later reviewable slices using the
same verified workflow.

**Post-inbox-handler decision gate:** Immediately after the inbox-handler
refactoring is complete, compare the existing NATS/PostgreSQL event spine alone
with self-hosted [Temporal](https://github.com/temporalio/temporal) and its Rust
SDK, [Duroxide](https://github.com/microsoft/duroxide), and
[Flawless](https://flawless.dev/), and
[DBOS Transact for Rust](https://github.com/dbos-inc/dbos-transact-rust). Use
the same synthetic, point-in-time M08 evaluation workflow to assess crash
recovery, deterministic replay, duplicate and cancellation behavior, workflow
evolution, Rust integration, operational footprint, licensing, maintenance,
cloud portability, and resource use. For DBOS, specifically prove that the
Rust-compatible enqueue path can commit or roll back a workflow with
service-owned application data in one PostgreSQL transaction; distinguish
that guarantee from transactional workflow steps and idempotency of external
effects. Record the selected option and rejected alternatives in an ADR before
implementing a workflow runtime. Do not begin model-provider selection, model
calls, or local model runtime experiments until this decision is recorded.
The gate is recorded in [ADR-0013](../decisions/0013-select-durable-workflow-execution.md):
self-hosted Temporal is selected for future long-running workflows, while the
NATS/PostgreSQL event spine and dry-run execution path remain unchanged. The
DBOS proof established same-transaction SQL enqueue, but did not establish
that the current Rust executor can consume those enqueued arguments.

## M03 - Market data fabric

**Outcome:** Publish trustworthy point-in-time evidence through provider-neutral contracts.

The [acquisition implementation plan](../development/plans/m03-acquisition.md)
orders the remaining daily work and separately bounded streaming extension.
M03-PR11A implements the invented offline source/P05 replay bridge, not service
activation or live qualification. The conditional daily-provider pair and its
unmet qualification requirements are recorded in that plan. Durable reviewed
daily-credit accounting must precede provider adapters; selection alone never
authorizes live activation, credentials or data use.

Work items:

- Define stable instrument identity, market time, observations, corporate actions, entitlements, and evidence snapshots.
- Add redistribution-safe synthetic fixtures covering holidays, splits, stale data, gaps, duplicates, and disagreement.
- Implement canonical calendar, OHLC, volume, freshness, and outlier validation.
- Publish content-addressed snapshots or typed quality failures through the event spine.
- Define the provider adapter contract and conformance suite.
- Implement one production-candidate daily-bar adapter behind the contract.
- Add timeouts, cancellation, rate-limit handling, provider revision, and error classification.
- Store licensed evidence in object storage under explicit retention and checksum policy.
- Keep live-provider tests opt-in and credential-safe.

Completion means synthetic and one live provider produce identical canonical
contracts and failure semantics without provider logic entering domain crates.

The [daily evidence profile](../development/market-data.md) retains its raw daily
scope and budgets. [ADR-0032](../decisions/0032-define-vendor-neutral-simulation-profiles.md)
defers vendor selection and adds separately qualified
[intraday/options profiles](../development/simulation-profiles.md). Provider
rights and actual live-daily-data qualification remain required; documentation
alone does not complete M03 or qualify the extensions. The
[P01-P13 delivery plan](../development/plans/simulation-expansion.md) integrates
derivative specifications, bars/trades/index observations, option snapshots and
continuity/recovery before their provider/schema consumers. It preserves the
approved daily-data sequence rather than implementing parallel ports.

## M04 - Deterministic research

**Outcome:** Publish the first reproducible artifact or abstention.

Work items:

- Define versioned Research Request, Research Artifact, Abstention, and lifecycle contracts.
- Implement one momentum calculation with declared lookback, units, adjustment basis, and numeric tolerance.
- Implement deterministic candidate eligibility, ranking, and tie-breaking.
- Add minimum-evidence policy with stable abstention reason codes.
- Render a deterministic artifact without a language model.
- Commit artifact state and its publication event atomically.
- Add replay tests for identical evidence and implementation revisions.
- Expose asynchronous submission and query through the gateway and audit projector.
- Demonstrate that duplicate requests create no duplicate artifact.

Completion means separate gateway, market-data, research, and projector processes
produce a reproducible artifact or correct abstention from offline evidence.

The expansion plan adds virtual time, deterministic input/ID/randomness manifests
(P05) and separately qualified Greeks/IV analytics (P11). P11 can proceed in
parallel and does not block quote-driven, fully funded simulation.

## M05 - Strategy and policy

**Outcome:** Evolve approved strategies without weakening independent controls.

Work items:

- Define strategy metadata, lifecycle, compatibility, approval, and validation evidence.
- Move momentum behind a strategy registry without changing its output.
- Define required features, eligibility, score, tie-break, horizon, expiry, benchmark, regime, and invalidation rules.
- Separate experimental and approved strategy execution.
- Implement strict versioned configuration with unknown-field rejection.
- Define policy inputs, decisions, immutable versions, and reason codes.
- Enforce instrument, direction, horizon, liquidity, volatility, gap, data-quality, and event constraints.
- Add artifact validation, numeric reconciliation, and audited kill switches.
- Record multiple-testing and promotion evidence before adding a second strategy.

Completion means strategy implementations can evolve independently while policy
and artifact validation remain deterministic and impossible to bypass.

P06 adds account-wide and per-strategy/session limits, valuation, breach latching
and authorized reset policy. ExecutionSimulator must enforce them atomically;
strategy approval cannot override the account boundary.

## M06 - Evidence-grounded explanation

**Outcome:** Explain approved research without allowing generated content to create facts.

Work items:

- Define the explanation proposal schema and evidence-citation contract.
- Build a policy-approved evidence allowlist for each artifact.
- Treat retrieved documents as untrusted content, never executable instructions.
- Implement one optional model adapter behind a model-neutral port.
- Reject uncited claims, altered values, unsupported identifiers, and invalid disclosures.
- Add prompt-injection, fabricated citation, conflicting instruction, and numeric mutation tests.
- Fall back to deterministic rendering for outage, timeout, budget, or validation failure.
- Pin model, settings, prompt, tools, and validator versions for audit.
- Evaluate groundedness and usefulness on a frozen suite.

Completion means a model can improve presentation but cannot change facts,
ranking, policy, publication, or order eligibility.

## M07 - Dry-run execution

**Outcome:** Process simulated orders, fills, positions, cash, and P&L without a live path.

Work items:

- Define `SubmitDryRunOrder` and append-only order, fill, ledger, and position events.
- Implement an execution simulator with only the literal `dry_run` mode.
- Reject missing, unsupported, unauthorized, stale, or policy-ineligible requests before acceptance.
- Support long-only market and limit orders with explicit time-in-force and cancellation.
- Version spread, slippage, fees, latency, participation, gap, halt, and corporate-action behavior.
- Atomically persist order transitions, fills, balanced ledger entries, and outbox events.
- Add duplicate-command, duplicate-observation, partial-fill, restart, and replay tests.
- Enforce egress policy and scan source, images, and configuration for broker credentials and live endpoints.
- Expose order and portfolio-simulation views through rebuildable projections.

Completion means an approved scenario yields reproducible simulated execution
state while architecture tests prove no live broker path exists.

P07-P10 add the event-applied order lifecycle, serialized account/reservation
boundary, evidence-driven fills/latency and qualified cash-settled European
index-style option settlement. Cash/physical and European/American metadata are
modeled earlier, but physical delivery and American exercise/assignment remain
unsupported in the initial simulation. Stable IDs and replaceable capability
seams do not authorize broker adapters or live execution. Account concurrency
and recovery proofs ship here, not deferred to M10.

## M08 - Durable projections and evaluation

**Outcome:** Rebuild lifecycle views and measure outcomes without hindsight.

Work items:

- Persist immutable artifact, order, fill, position, policy, and lifecycle histories.
- Build projections that tolerate duplicates and resume from checkpoints.
- Rebuild every projection from authoritative state and retained events.
- Define point-in-time evaluation manifests and chronological holdouts.
- Include delistings, corporate actions, calendars, universe history, and declared fill timing.
- Report sample size, uncertainty, calibration, drawdown, turnover, cost sensitivity, and regime breakdown.
- Link evaluation and dry-run outcomes without mutating original artifacts.
- Verify expand-migrate-contract database evolution and restore procedures.
- Prevent evaluation output from becoming a performance claim without explicit review.

Completion means state survives restarts, read models are disposable, and research
and dry-run outcomes remain reproducible and explicit about uncertainty.

P12-P13 extend this outcome to account/order/risk projection rebuilds, canonical
simulation replay and frozen cost/latency/liquidity sensitivity evaluation.
P12's [account audit](../development/account-audit.md) supplies bounded owner
exports and checkpointed revision replay for finite synthetic runs, not public
projector activation or completion of all evaluation/restore work.
P13's [frozen evaluation](../development/frozen-evaluation.md) adds bounded
chronological synthetic fidelity and explicit one-factor sensitivity reports;
empirical calibration, public transport and durable evaluation publication remain.
Deployment, operational and UI follow-through remains in M09-M11 as specified
by the expansion plan; no new milestone or active-active account writer is added.

## M09 - Multi-cloud deployment profiles

**Outcome:** Promote identical signed artifacts through portable and managed profiles.

Work items:

- Package the reference Kubernetes profile with Knative or Deployments, KEDA, NATS JetStream, CloudNativePG, object storage, and OpenTelemetry.
- Keep Argo CD Applications, environment overlays, dependency instances, and immutable release selection in `edge-agent-gitops`.
- Implement OpenTofu modules with thin provider compositions and stable logical outputs.
- Qualify one managed-cloud profile first, then add GCP, Azure, and AWS profiles incrementally.
- Use workload identity and external secret references; prohibit static cloud credentials.
- Run identity, messaging, storage, telemetry, migration, restart, rollback, and restore conformance tests per profile.
- Promote the same signed image digests; never rebuild per provider.
- Document availability zones, region, residency, quotas, egress, retention, cost ceilings, and recovery objectives.
- Run scheduled drift tests against real provider infrastructure outside default CI.
- Demonstrate a portable Kubernetes deployment and one managed deployment from a tagged release.

Completion means every claimed profile satisfies the published portability
contract with evidence from the same automated suite.

## M10 - Operational readiness

**Outcome:** Operate, recover, limit, and stop the distributed system safely.

Work items:

- Enforce service identity, least privilege, broker ACLs, database ownership, and operator roles.
- Add bounded-cardinality metrics, correlated traces, structured logs, and immutable decision audit views.
- Enforce request, symbol, lookback, provider, model, concurrency, queue, storage, and execution budgets.
- Add readiness, liveness, graceful drain, backpressure, and explicit normal/degraded/fail-closed reporting.
- Define SLOs for availability, command acceptance, event age, evidence freshness, publication, dry-run fills, and projection lag.
- Add staged rollout, compatibility, rollback, quarantine replay, artifact withdrawal, and kill-switch runbooks.
- Verify backup, point-in-time restore, retention, deletion, and cross-profile recovery drills.
- Complete threat modeling, dependency/license review, failure injection, load tests, and production-readiness assessment.
- Publish an operator demo that diagnoses and recovers from a poisoned message, service crash, and projection rebuild.

Completion means operators can observe, constrain, recover, and stop the system
without a language model, broker account, or manual database repair.

## M11 - Trusted user experience

**Outcome:** Expose research and dry-run workflows without moving domain authority into the browser.

Work items:

- Accept an `edge-agent-ui` architecture decision covering the language,
  framework, rendering model, package policy, authentication, and artifact
  format.
- Generate and pin a typed client from gateway-owned API schemas; prohibit
  handwritten copies of domain or transport contracts.
- Submit research requests and expose durable acceptance, progress, terminal
  failure, and retry-safe status.
- Render research artifacts, abstentions, evidence provenance, timestamps,
  units, versions, uncertainty, and lifecycle state.
- Present generated explanations only with validated citations and safe content
  rendering.
- Submit market and limit scenarios using only the literal `dry_run` mode and
  keep authorization and validation authoritative in backend services.
- Display order transitions, fills, positions, cash, fees, and simulated P&L
  from projector-owned views without browser-side financial calculations.
- Distinguish loading, delayed, degraded, rejected, stale, withdrawn, and
  unavailable states instead of inventing continuity.
- Implement OIDC login, authorization-aware navigation, content security,
  dependency policy, accessibility, and responsive behavior.
- Add deterministic component, contract, browser, and end-to-end tests using
  synthetic fixtures and failure cases.
- Package the UI as an independently versioned, signed artifact and promote its
  immutable digest through `edge-agent-gitops`.
- Keep Argo CD, infrastructure mutation, credential administration,
  dead-letter replay, and emergency operations outside the initial user UI.

Completion means a user can submit an offline synthetic research request,
inspect a reproducible artifact or explicit abstention, submit an authorized
dry-run order, and follow its simulated lifecycle through accessible views.
Tests must prove the UI cannot create facts, bypass policy, reach internal
services, or request a live execution mode.

## Planning rules

- GitHub owns live issue state, assignee, labels, and milestone assignment.
- This document owns intended sequencing until an issue is created.
- The repository-policy checker records M11 as the published milestone
  high-water mark. Publishing M12 requires its index row, matching boundary,
  non-empty outcome, and checker constant in the same change.
- The milestone index uses one Markdown table whose first rendered row is the
  `Milestone | Outcome` header; every milestone is a body row in that table.
- Every issue names one exact milestone and one observable outcome.
- Each work item should fit one independently reviewable pull request.
- Later milestones may begin discovery early, but implementation cannot bypass an unmet integrity dependency.
- A profile or capability is supported only after its acceptance tests pass in automation.
