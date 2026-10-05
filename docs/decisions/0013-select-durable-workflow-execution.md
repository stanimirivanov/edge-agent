# ADR-0013: Select Temporal for durable workflow execution

- Status: Accepted
- Date: 2026-10-05
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Select self-hosted Temporal and its Rust SDK for future long-running M08
evaluation workflows. Keep NATS JetStream and PostgreSQL as the cross-service
event spine; Temporal does not replace command/event contracts, inbox/outbox
atomicity, or service ownership. A workflow starter will bridge committed
outbox intent to a stable Temporal workflow ID. This decision selects an
execution model, not a model provider or a production runtime implementation.

Completed activity results survive worker death and replay skips their code.
An activity killed while an external request is in flight can run again; no
candidate can promise exactly-once effects at an opaque model provider merely
by replaying local history. Model-facing activities therefore require stable
effect IDs, provider idempotency where available, and reconciliation where it
is not. The executable Temporal crash/replay proof passed; production
PostgreSQL-backed Kubernetes behavior remains a later deployment gate.

## Context

The existing spine proves durable *message* delivery and one committed domain
transition under redelivery. It does not persist a program counter or completed
step results for a multi-step evaluation. Running a model call inside the
inbox transaction would hold a database transaction across unbounded external
I/O and violate the inbound adapter's contract. Running it after the inbox
commit and broker acknowledgement loses the continuation if the worker dies.
The [M08 negative control](../../experiments/event-spine-proof/tests/m08.rs)
reproduces that boundary with the real adapters.

The durable-execution requirement is narrower and stronger than message
deduplication: after a worker dies during synthetic Step B, restart the same
workflow, do not invoke completed Step A again, retry or reconcile Step B, and
publish one terminal result. Input identifies an immutable point-in-time
snapshot; replay must not observe current market data, a mutable policy, or a
new model response for a previously completed activity. Real providers and
local model runtimes are outside this decision and were not contacted.

The evaluation uses the same logical A/B scenario across isolated Rust tests.
It weighs crash recovery, deterministic replay, duplicate/cancellation
semantics, workflow evolution, Rust integration, operating and resource
footprint, open-source licensing, maintenance, and deployment on GCP, Azure,
AWS, or portable Kubernetes. A documented API is not marked as an executable
proof where no test was run.

### Decision matrix

| Criterion | Existing NATS/PostgreSQL spine alone | Temporal Rust SDK | Duroxide | Flawless | DBOS Transact for Rust |
| --- | --- | --- | --- | --- | --- |
| Crash at Step B / completed A | **Fails** as a workflow: A commits once, B has no continuation after acknowledgement | **Passes** isolated child-process abort/restart; A=1 and completed B, followed by history replay | **Passes** isolated SQLite child-process kill/restart; A=1, B=2 | **Passes for recorded effects** on Windows: server kill/restart yields A HTTP effect=1, B attempts≥2, C effect=1; pure local A code is not protected from replay | **Passes** native Rust SDK abort/restart on PostgreSQL; A=1, B=2. This does not establish SQL-enqueued workflow compatibility |
| Replay and evolution | Requires a new state machine, step ledger, and version rules | Server history plus Rust `WorkflowReplayer`; `ctx.patched` supports compatible code changes | Recorded orchestration history; isolated test keeps a running v1 instance on v1 with v2 registered | Deterministic code re-executes on replay while recorded effects are skipped; effect-boundary recovery tested, version-evolution conformance **not run** | SDK records steps; Rust serializer/interop and transactional-step gaps remain open |
| Duplicates and cancellation | Inbox/outbox identity and bounded redelivery; no whole-workflow cancellation | Stable workflow IDs and server cancellation; production cancellation/duplicate race tests still required | Duplicate pre-start was accepted but did not replace input or repeat A; in-flight B cancellation reached terminal failure in test | B retry requires an explicit idempotency declaration; duplicate-start and cancellation behavior **not run** | Stable SQL workflow ID is available; native Rust cancellation and SQL-enqueued Rust execution **not run** |
| Rust integration | Existing native adapters, but orchestration must be built | Official Rust SDK 1.0.0 and native workers; newer surface than other Temporal SDKs | Native async Rust 0.1.30, embedded runtime | Rust workflows compile to Wasm; separate server/deploy toolchain | Native Rust SDK exists, but SQL `portable_json` enqueue cannot currently be assumed consumable by it |
| Operations and resource use | Already runs NATS + PostgreSQL; custom workflow engineering/operations cost | Additional self-hosted server services, persistence, workers, backup and upgrades; highest topology footprint | Embedded worker plus SQLite or PostgreSQL provider; lower topology footprint | Separate closed-source server plus Wasm build/deploy path | Embedded worker plus PostgreSQL; low additional topology footprint |
| License, maintenance, clouds | Existing open-source components; already portable | MIT server/SDK; active upstream; Helm supports self-hosting with PostgreSQL on portable Kubernetes | MIT, active but explicitly **preview**; PostgreSQL provider is separately versioned | Library BSD-2-Clause-Patent; server is closed source; not acceptable as an auditable open-source infrastructure dependency | MIT, active early Rust port; PostgreSQL is cloud portable but open Rust capability gaps block adoption |

Topology is a resource-use proxy, not a CPU, memory, throughput, or cost
benchmark. No controlled resource benchmark was run, so this ADR does not
invent one. Temporal's extra control-plane footprint is an accepted cost only
for workflows that need durable step history; ordinary event consumers stay on
the existing spine. The official [Temporal Rust SDK](https://github.com/temporalio/sdk-rust/blob/main/crates/sdk/README.md),
[Temporal Helm chart](https://github.com/temporalio/helm-charts/blob/main/README.md),
[Duroxide preview](https://github.com/microsoft/duroxide),
[Flawless installation terms](https://flawless.dev/docs/installation/), and
[DBOS Rust serializer issue](https://github.com/dbos-inc/dbos-transact-rust/issues/45)
support the documentary entries; executable evidence is linked below.

### Automated proof results

| Proof | Result on 2026-10-05 | Assertion and limit |
| --- | --- | --- |
| [Event-spine negative control](../../experiments/event-spine-proof/tests/m08.rs) | Passed on Windows with temporary PostgreSQL and NATS testcontainers | Kill child after the real inbound transaction and acknowledgement, at B entry. Restart sees no pending delivery; A=1, B-started=1, B-completed=0. This proves the *spine alone* has no continuation, not that an added state machine could never work. |
| [Duroxide crash/evolution/duplicate/cancel](../../experiments/durable-execution/tests/duroxide_crash_recovery.rs) | Two tests passed on Windows with temporary file SQLite | Kill child at B, restart same instance: A invocation=1, B attempts=2, completed result. Conflicting pre-start admission is accepted but does not change the original input. V1 stays v1 with v2 registered. Cancellation is terminal; PostgreSQL provider and actual HTTP cancellation remain untested. |
| [DBOS atomic SQL enqueue](../../experiments/dbos-proof/tests/atomic_enqueue.rs) | Passed on Windows with a digest-pinned official DBOS/PostgreSQL testcontainer | In one PostgreSQL transaction, app row plus `dbos.enqueue_workflow` row both roll back after a forced uniqueness error and both survive commit. The committed row is `ENQUEUED` on the expected queue with `portable_json` serialization. This proves atomic enqueue, **not** Rust-worker consumption, transactional workflow steps, or exactly-once external effects. |
| [DBOS native Rust crash/restart](../../experiments/dbos-proof/tests/native_recovery.rs) | Passed on Windows with a digest-pinned official DBOS/PostgreSQL testcontainer | First worker aborts inside native SDK Step B; restarted worker completes the original workflow with A runs=1, B runs=2, terminal `SUCCESS`, and `rust_serde` serialization. This tests native SDK start/recovery, not the separate SQL enqueue path. |
| [Temporal Rust SDK crash/replay](../../experiments/temporal-proof/tests/crash_recovery.rs) | Passed on Windows with Temporal CLI 1.8.3 / Server 1.31.2 | Child aborts inside B; restarted worker completes the original instance with A invocation=1, B completed, and actual server history replay succeeds. The test uses an ephemeral SQLite-backed dev server, not a production PostgreSQL-backed cluster. Its CLI archive is version- and SHA-256-pinned before execution. |
| [Flawless recorded-effect crash/replay](../../experiments/flawless-proof/src/tests.rs) | Passed on Windows with a digest-pinned official beta.3 server binary | Kill the server while the synthetic B HTTP effect is in flight and restart from the same working directory: A HTTP effect=1, B attempts≥2, C effect=1. This proves recorded-effect recovery, **not** the literal requirement that pure local Step A code never re-executes; Flawless replays deterministic code. Linux CI has not yet run. |

The DBOS test calls the PostgreSQL function through Rust's `tokio-postgres`
client. PostgreSQL provides the commit/rollback guarantee because both writes
use one connection and transaction. The tests observe `portable_json` on the
SQL-enqueued row and `rust_serde` on the native Rust row. [DBOS Rust issue #45](https://github.com/dbos-inc/dbos-transact-rust/issues/45)
documents the Rust SDK's missing portable serializer, so SQL row existence is
not equivalent to a Rust worker executing that row. [Issue #48](https://github.com/dbos-inc/dbos-transact-rust/issues/48)
separately documents missing transactional workflow steps. Neither feature
would make an external model call atomically roll back with PostgreSQL.

## Decision

Use self-hosted Temporal as the sole long-running workflow runtime when M08
workflow implementation begins. Retain NATS/PostgreSQL for cross-service
commands and facts and for short, idempotent consumers. Do not replace
service-owned databases or publish Temporal history as a domain event.

The integration will have these boundaries:

1. A service transaction writes application state and a versioned workflow-start
   intent to its existing PostgreSQL outbox. It performs no Temporal or model
   network call while the transaction is open.
2. The outbox relay publishes the intent to the event spine. A dedicated
   workflow starter validates it and starts Temporal with a deterministic
   workflow ID derived from the authoritative request identity. On ambiguous
   start failures, it queries or retries using that same ID; duplicates must
   join the existing execution, not create another evaluation.
3. The workflow receives a frozen manifest ID and versioned policy references,
   not mutable market facts. Deterministic orchestration schedules Step A and
   Step B as activities. Completed activity results are in durable history;
   replay does not invoke A again. Activity code owns timeouts, bounded retry,
   cancellation, safe request identifiers, and external-effect reconciliation.
4. A terminal activity commits the result or abstention and a publication
   event through the service's PostgreSQL outbox. Its write is idempotent by
   workflow/result identity. Transport redelivery cannot create another
   authoritative artifact.

The first production slice must prove the starter bridge, ambiguity handling,
workflow ID collision policy, history compatibility, cancellation, and a
multi-worker deployment with persistent PostgreSQL-backed Temporal. The local
SDK dev server is only an executable semantics probe, not an operational
acceptance test for self-hosted Kubernetes.

## Alternatives considered

### Existing event spine alone

- Benefits: No new server or dependency; strong message-level identity,
  transactional outbox/inbox, and proven one-transition delivery.
- Costs and risks: Step checkpointing, timers, replay determinism, cancellation,
  version routing, and workflow observability would become custom platform
  code. The negative control loses B after A is acknowledged.
- Reason not selected: Its existing guarantees solve event delivery, not
  long-running execution. It remains a necessary complementary backbone.

### Duroxide

- Benefits: The executable recovery proof passes; native Rust, embedded
  runtime, low operational footprint, SQLite for tests, PostgreSQL provider for
  deployment, and versioned orchestration support.
- Costs and risks: Upstream calls it preview. Its PostgreSQL provider,
  multi-worker fencing, schema upgrades, and actual external-call cancellation
  are not covered by this evaluation. The test observed duplicate pre-start
  acceptance, requiring downstream identity checks.
- Reason not selected: A preview runtime and an unproven production storage
  profile are a weaker foundation for EdgeAgent's most critical durability
  guarantee than Temporal's server-managed history. Re-evaluate if its
  production contract and PostgreSQL conformance mature.

### Flawless

- Benefits: Rust-authored, versioned Wasm workflows. The automated server
  crash test proves recovery of recorded HTTP effects.
- Costs and risks: The server binary is downloadable but closed source. Its
  replay re-executes deterministic code and suppresses previously recorded
  side effects, so a literal local Step A does not meet the no-reexecution
  assertion without a durable effect boundary. The test does not cover pure
  local-code replay, cancellation, or version evolution. Wasm and a separate
  deployment path add integration work.
- Reason not selected: An open-source reference project cannot make its
  critical durability guarantee depend on an opaque runtime whose replay
  semantics require effect-boundary adaptation to satisfy M08's literal Step A
  constraint. Its distribution and operations contract is also less suitable
  than an auditable self-hosted control plane.

### DBOS Transact for Rust

- Benefits: The SQL enqueue and application row really commit or roll back in
  one PostgreSQL transaction. Embedded runtime and PostgreSQL align with the
  project's portable data plane.
- Costs and risks: The verified SQL enqueue path emits `portable_json`; the
  current Rust SDK does not yet implement that serializer. A committed queue
  row therefore does not establish Rust executor compatibility. The separate
  native Rust crash/restart proof passes, but it does not start from the SQL
  function's row. The Rust SDK also lacks transactional workflow steps;
  cancellation and evolution of a SQL-enqueued workflow remain unproven.
- Reason not selected: Its strongest differentiator is real but incomplete
  for an end-to-end Rust workflow until enqueue-to-executor compatibility and
  crash recovery pass in one automated suite. Re-evaluate when those gaps
  close; keep atomic enqueue as an explicit requirement for any future switch.

## Consequences

### Positive

- Step-level recovery and deterministic history replay become a framework
  contract rather than a bespoke state-machine obligation in each service.
- Existing event contracts, service ownership, and dry-run-only behavior stay
  unchanged. Workflow adoption can be scoped to genuinely long-running work.
- Temporal can run in each cloud on the same Kubernetes and PostgreSQL profile;
  no managed workflow service is required.

### Negative

- A self-hosted Temporal control plane adds deployment, persistence, schema
  upgrade, backup/restore, authentication, telemetry, and capacity obligations.
- Starting a Temporal workflow cannot share the service's PostgreSQL commit.
  The outbox bridge is eventual and must close ambiguous-start races with a
  stable ID and reconciliation.
- A failed or timed-out model activity may repeat a provider request. The
  workflow history alone cannot make that external effect exactly once.

### Neutral or follow-up

- Quantify memory, CPU, latency, history growth, and PostgreSQL capacity on
  the portable Kubernetes profile before production rollout; no resource
  threshold is claimed from the synthetic tests.
- Re-test Duroxide and DBOS after maturity or serializer changes only through
  the same crash, duplicate, cancellation, and versioning contract.
- No model provider or local runtime is selected by this ADR.

## Compatibility and migration

There is no production workflow history to migrate and no change to published
CloudEvents or dry-run order contracts. The first workflow-start intent needs
a new versioned event definition; old consumers must ignore or reject it by
their existing routing rules. Rollout starts with a new dedicated starter and
worker, gated by synthetic traffic. Rollback stops new starts, drains or
cancels existing workflow instances according to an explicit operator policy,
and keeps the event spine and service data intact. Never discard open history
or deploy incompatible workflow code without patching/versioned workers.

## Security and operations

Keep Temporal payloads to opaque manifest IDs, version references, bounded
status, and correlation identifiers. Store licensed evidence, private prompts,
responses, and credentials in their authorized stores, not unbounded workflow
history or logs. Give M08 a dedicated Temporal namespace; configure TLS,
authentication, a non-noop default JWT claim mapper and Authorizer, secret
rotation, retention, encrypted backups, and audit logs in every cloud profile.
Prove that starter and worker identities cannot access other namespaces. The
stock Authorizer grants namespace-level roles; it does not restrict a starter
to one workflow type or a worker to one task queue. Do not place unrelated
workloads in the M08 namespace. If co-tenancy or a stricter threat model later
requires type/queue permissions, deploy a custom Authorizer that inspects the
request target and prove denial of out-of-scope starts and polls before that
rollout. Limit service-owned data access independently through database and
network credentials. See Temporal's
[self-hosted security guidance](https://docs.temporal.io/production-deployment/self-hosted-guide/security)
and [Authorizer interface](https://github.com/temporalio/temporal/blob/main/common/authorization/authorizer.go).

The production runbook must cover unavailable Temporal server or persistence,
stuck queues, activity timeout, heartbeats, retry exhaustion, cancellation,
history growth, schema upgrades, restore, and operator replay. An unavailable
workflow starter leaves the committed intent in the outbox/event spine and
alerts on age; it does not invent a result or bypass policy. Cancellation is
cooperative and cannot undo an already accepted external request. Dry-run
execution remains physically unable to transmit a live order.

The test launcher verifies the version-pinned Temporal CLI archive against a
checked-in SHA-256 taken from the upstream release checksum list before
execution. The Flawless proof likewise verifies a checked-in digest observed
from its official HTTPS artifact, but that digest is not vendor-signed. In
production, pin and verify server container images and their provenance as
part of the deployment supply chain; these test-binary checks alone are not
production artifact attestation.

## Validation

- Run each isolated proof with `cargo test --locked` from its own directory;
  see [the experiment guide](../../experiments/README.md). Docker-backed tests
  self-provision their dependencies; the Temporal and Flawless tests download
  and verify pinned server binaries on each run.
- The Temporal subprocess crash test passed with A=1, B completed after
  restart, and replay of the server history. Re-run it on Linux CI before a
  production implementation is accepted.
- The Flawless effect-boundary crash test passed on Windows. Run it on Linux CI;
  do not reinterpret an A HTTP-effect count of one as proof that pure local
  workflow code was skipped.
- Before implementing model calls, add server-backed Linux CI evidence for
  duplicate start, cancellation, activity timeout/heartbeat, versioned worker
  rollout, and ambiguous-start reconciliation. Existing positive tests do not
  yet cover all of these production failure modes.
- Before deploying Temporal, run a persistent PostgreSQL-backed Kubernetes
  smoke test plus backup/restore, security, and capacity qualification in the
  portable profile. Only then promote equivalent GCP, Azure, and AWS profiles.
