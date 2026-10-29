# EdgeAgent documentation map

## TL;DR

- Start with [CONTRIBUTING.md](../CONTRIBUTING.md), then use this map to read
  only the sources relevant to the task.
- Product intent, architecture, engineering practice, operations, roadmap, and
  durable decisions have separate canonical homes.
- Update this map when a canonical source moves or a recurring task lacks a
  clear route.
- Keep API detail in rustdoc; add a focused guide only when contributors or
  operators need a workflow that code cannot explain.

## Purpose and authority

For finite synthetic stream execution, exact history recovery and borrowed domain
readiness, read [offline stream runtime](development/offline-stream-runtime.md) and
[ADR-0074](decisions/0074-compose-finite-offline-streams-behind-durable-restore-fences.md).

For receipt-bound native streaming metadata, paired recovery and cleanup, read
[native streaming provenance](development/stream-native-provenance.md) and
[ADR-0073](decisions/0073-bind-native-stream-companions-to-publication-receipts.md).

For independently installed streaming inputs, restricted worker loading and retained
namespace ownership, read [stream owner inputs](development/stream-owner-inputs.md)
and [ADR-0072](decisions/0072-authenticate-stream-owner-inputs-and-reserve-namespaces.md).

For explicit owner-pinned IEX conditions, canonical identities and native provenance,
read [IEX canonical acceptance](development/iex-canonical-acceptance.md) and
[ADR-0071](decisions/0071-pin-iex-canonical-interpretation-to-complete-owner-profiles.md).

For offline fair bounded stream queues and unresolved dispatch ownership, read
[stream supervision](development/stream-supervision.md) and
[ADR-0069](decisions/0069-bound-offline-stream-supervision-with-unresolved-heads.md).
For protected incidents, independent restore approval and exact pending recovery,
also read [ADR-0070](decisions/0070-fence-stream-restores-with-durable-incident-barriers.md).

For offline Alpaca IEX trades/minute bars and exact bounded subscriptions, read
[Alpaca IEX](development/alpaca-iex.md) and
[ADR-0068](decisions/0068-qualify-iex-streams-with-exact-offline-subscriptions.md).

For bounded WSS frames, verified TLS, durable arrival charging and offline protocol proofs,
read [streaming transport](development/streaming-transport.md) and
[ADR-0067](decisions/0067-bound-wss-frames-before-provider-decoding.md).

For bounded normalized streaming capture (not evidence readiness), read
[streaming segments](development/streaming-segments.md) and
[ADR-0065](decisions/0065-separate-stream-capture-segments-from-evidence-readiness.md).

For finite offline streaming authority, explicit budgets and durable account slots,
read [streaming sessions](development/streaming-sessions.md) and
[ADR-0064](decisions/0064-bound-streaming-sessions-with-durable-account-fences.md).

For owner-reviewed finite daily qualification profiles and fail-closed offline checks,
read [qualification preflight](development/qualification-preflight.md) and
[ADR-0061](decisions/0061-separate-owner-reviewed-qualification-preflight-from-execution.md).
For independent PostgreSQL owner grants and atomic worker mutation guards, read
[qualification owner control](development/qualification-owner.md) and
[ADR-0062](decisions/0062-require-independent-postgresql-owner-grants-for-qualification.md).
For bounded vendor workers, complete owner inputs, persistent fencing and exact approved
cleanup, read [qualification runtime](development/qualification-runtime.md) and
[ADR-0063](decisions/0063-compose-owner-authorized-finite-vendor-qualification.md).
For opt-in command supervision, honest reports and the remaining private live evidence,
read the [qualification harness](development/qualification-harness.md).

For unattended M02 crash/restart/wire-ACK qualification, use the focused
[qualification guide](development/m02-qualification.md).

This page is the progressive-disclosure entry point for contributors and
coding agents. It routes readers to canonical sources instead of repeating
their policy. [CONTRIBUTING.md](../CONTRIBUTING.md) owns workflow, issue shape,
verification, and completion reporting. [AGENTS.md](../AGENTS.md) is the
concise tool-facing agreement.

Read the rows that match the work. Do not bulk-read every guide or decision.
Always consult the product and architecture sources when a change affects
observable behavior, evidence meaning, ownership, or a dependency boundary.

For M04 versioned research payloads, read [research contracts](development/research-contracts.md).

For M04 frozen evidence eligibility, read [research evidence](development/research-evidence.md).

## Route by task

For native daily quality, explicit owner observation lineage and accepted object
compatibility, read [native daily acceptance](development/native-daily-acceptance.md)
and [ADR-0059](decisions/0059-accept-native-daily-evidence-with-explicit-owner-lineage.md).
For native preparation recovery, scoped storage and shared terminal transactions,
also read [ADR-0060](decisions/0060-persist-native-preparations-and-publish-through-shared-terminal-fences.md).

For evidence retirement, immediate durable denial, opaque account blocks and orphan
deletion, read [evidence retention](development/evidence-retention.md) and
[ADR-0053](decisions/0053-retire-evidence-with-opaque-denials-and-quiescent-cleanup.md).

For pinned instrument selection, narrowed provider requests and header-first routing,
read [instrument selection](development/instrument-selection.md) and
[ADR-0052](decisions/0052-freeze-instrument-selection-and-route-headers-before-payloads.md).

For permit-bound provider pages and usage projection, read
[provider HTTP](development/provider-http.md) and
[ADR-0051](decisions/0051-bind-provider-page-dispatch-to-durable-permits.md).

For invented daily provider output, strict offline selection and deterministic
continuations, read [offline market data](development/offline-market-data.md),
[ADR-0050](decisions/0050-isolate-invented-daily-sources-and-reuse-frozen-replay.md)
and the [M03 acquisition plan](development/plans/m03-acquisition.md).

For local-only daily runtime composition, configuration, receipt recovery and
owner-confirmed maintenance, read the [offline worker](development/offline-worker.md)
and [ADR-0054](decisions/0054-compose-an-offline-only-acquisition-worker.md).

For M03 atomic terminal receipt/inbox/outbox publication and correction fencing, read
[acquisition publication](development/acquisition-publication.md) and
[ADR-0049](decisions/0049-commit-terminal-acquisition-publication-atomically.md).

For M03 immutable object persistence and bounded integrity reads, read
[evidence storage](development/evidence-storage.md) and
[ADR-0047](decisions/0047-store-scoped-evidence-with-create-only-integrity-checks.md).

For durable admission and shared provider accounting, read the
[acquisition ledger guide](development/acquisition-ledger.md) and
[ADR-0048](decisions/0048-persist-acquisition-admission-and-call-budgets.md).
For reviewed daily credit allocation, midnight recovery and fail-closed installation,
also read [ADR-0055](decisions/0055-reserve-reviewed-daily-credits-with-account-permits.md).

For M03 exact numeric provider parsing and bounded HTTP, read
the [Twelve Data native protocol](development/twelve-data.md),
the [Alpaca precise historical protocol](development/alpaca.md),
[provider HTTP](development/provider-http.md) and
[ADR-0046](decisions/0046-bound-provider-http-and-preserve-exact-json-tokens.md).

For M03 versioned evidence bytes, lineage and scoped terminal messages, read
[evidence artifacts](development/evidence-artifacts.md) and
[ADR-0045](decisions/0045-freeze-versioned-evidence-artifacts-and-terminal-contracts.md).
For the native-date temporal prerequisite and explicitly reviewed UTC bucket provenance,
also read [ADR-0056](decisions/0056-preserve-native-dates-with-reviewed-bucket-provenance.md).

For M03 acquisition requests, reviewed rights and provider conformance, read
[acquisition authority](development/market-data-acquisition.md) and
[ADR-0044](decisions/0044-gate-acquisition-on-reviewed-rights-and-provider-authority.md).

For P13 frozen-run fidelity and explicit cost/latency/liquidity sensitivity, read
[frozen evaluation](development/frozen-evaluation.md) and
[ADR-0042](decisions/0042-freeze-complete-synthetic-sensitivity-evaluations.md).

For P12 account/order/risk audit exports, rebuilds and checkpoints, read
[account audit](development/account-audit.md) and
[ADR-0041](decisions/0041-retain-owner-authored-account-audit-revisions.md).

For P11 theoretical Greeks and bounded implied volatility, read
[option analytics](development/option-analytics.md) and
[ADR-0040](decisions/0040-qualify-bounded-european-option-analytics.md).

For P10 synthetic final fixings and durable account cash settlement, read
[option settlement](development/option-settlement.md) and
[ADR-0038](decisions/0038-bind-synthetic-cash-settlement-to-explicit-policy.md) and
[ADR-0039](decisions/0039-commit-account-settlement-as-a-canonical-postlude.md).

For P08 atomic account admission and synthetic reservations, read
[account admission](development/account-admission.md) and
[ADR-0036](decisions/0036-serialize-synthetic-account-admission-and-reservations.md).

For P09 fills, cumulative fees/book cost, shared liquidity and virtual latency,
read [evidence execution](development/evidence-execution.md) and
[ADR-0037](decisions/0037-commit-evidence-driven-simulation-execution.md).

For P07 dry-run order facts, cancellation/expiry races and the simulation port, read
[dry-run orders](development/dry-run-orders.md) and
[ADR-0035](decisions/0035-apply-dry-run-order-lifecycles-from-retained-facts.md).

For P06 account/strategy/session risk, breach latches and authorized recovery, read
[simulation risk](development/simulation-risk.md) and
[ADR-0034](decisions/0034-enforce-versioned-simulated-account-risk-policy.md).

For P05 manifests, virtual time, deterministic IDs/RNG and restart state, read
[deterministic runs](development/deterministic-runs.md) and
[ADR-0033](decisions/0033-freeze-deterministic-run-continuations.md).

| Task or changed area | Read before changing it | Verify or consult as needed |
|:--|:--|:--|
| Product behavior, vocabulary, evidence meaning, or non-goals | [Product vision](product/vision.md) | [System architecture](architecture/system-overview.md), relevant [ADRs](decisions/README.md) |
| Capability ownership, crate roles, or dependency direction | [System architecture](architecture/system-overview.md), [dependency rules](architecture/dependency-rules.md) | [Engineering standards](development/engineering-standards.md), `make architecture` |
| Rust implementation, API design, or tests | [Engineering standards](development/engineering-standards.md) | Relevant capability guide, `make repository`, then `make verify` |
| Event contracts, routing, consumers, outbox, or inbox | [Message contracts](development/message-contracts.md), [messaging adapters](development/messaging-adapters.md) | [Event-spine conformance](development/event-spine-conformance.md), relevant persistence guide |
| Registry-derived broker topology, drift, or runtime permissions | [JetStream topology](development/jetstream-topology.md) | [M02 completion plan](development/plans/m02-completion.md), `cargo xtask verify --check nats-topology` |
| Independent process acceptance, private schemas, or worker lifecycle | [Process acceptance](development/process-acceptance.md) | [M02 completion plan](development/plans/m02-completion.md), `cargo xtask verify --check event-spine-processes` |
| Authorized quarantine replay, dispatch recovery, or operator inspection | [Quarantine replay](development/quarantine-replay.md) | [ADR-0028](decisions/0028-compose-authorized-recoverable-quarantine-replay.md), `cargo xtask verify --check event-spine-processes` |
| Event-spine export, W3C links, bounded metrics, or exporter degradation | [Event-spine telemetry](development/event-spine-telemetry.md) | [ADR-0029](decisions/0029-export-bounded-correlated-event-spine-observations.md), `cargo xtask verify --check event-spine-processes` |
| Images, dependencies, releases, or artifact provenance | [Supply chain](development/supply-chain.md), [release guide](development/releases.md) | `make supply-chain`, `make image-smoke` when applicable |
| Local runtime dependencies or recovery tests | [Local platform](development/local-platform.md) | `make local-up`, relevant conformance guide |
| Deployment ownership or cloud portability | [Deployment portability](architecture/deployment-portability.md) | [System architecture](architecture/system-overview.md), relevant ADRs |
| Repository policy, CI, agent guidance, or developer experience | [Coding harness](development/harness.md) | `make repository`, `make architecture` |
| Workflow capabilities, action pins, or credential detection | [CI security](development/ci-security.md), [security policy](../SECURITY.md) | `cargo xtask workflow-security`, `cargo xtask verify ci-security` |
| Milestone or issue planning | [Milestones](roadmap/milestones.md), [CONTRIBUTING.md](../CONTRIBUTING.md) | Relevant product and architecture sources |
| M03 market evidence, provider boundaries, rights or acquisition limits | [Market-data profile](development/market-data.md) | [ADR-0032](decisions/0032-define-vendor-neutral-simulation-profiles.md), [M03 scope](roadmap/milestones.md#m03---market-data-fabric) |
| Intraday/options profiles, simulation eligibility, account consistency or expansion sequencing | [Simulation profiles](development/simulation-profiles.md) | [Delivery plan](development/plans/simulation-expansion.md), [ADR-0032](decisions/0032-define-vendor-neutral-simulation-profiles.md) |
| Vulnerability reporting or security-sensitive behavior | [Security policy](../SECURITY.md) | Relevant architecture and ADRs |

## Canonical collections

- [Product](product/vision.md) defines purpose, users, scope, and non-goals.
- [Architecture](architecture/system-overview.md) defines system boundaries,
  ownership, trust, and information flow.
- [Development](development/engineering-standards.md) defines implementation
  and testing practices; focused guides define individual workflows.
- [Architecture decisions](decisions/README.md) record durable choices and
  lifecycle history.
- [Roadmap](roadmap/milestones.md) groups intended delivery into outcomes.
- [Security](../SECURITY.md) defines vulnerability reporting and security
  expectations. [CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md) defines community
  behavior.

## Keeping the map useful

A change updates this page when it adds, moves, renames, or removes a canonical
source or creates a recurring task without a route. Avoid listing short-lived
implementation details. `cargo xtask repository` checks mechanical document
policy and repository-local navigation; semantic accuracy and stale guidance
still require review.
