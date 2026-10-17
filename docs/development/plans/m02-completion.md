# M02 event-spine completion plan

## TL;DR

- Milestone: **M02 - Contracts and event spine**.
- Build on completed contracts, messaging, inbox/outbox, handler, and relay
  refactors; do not repeat them.
- Use synthetic acceptance contracts and independent proof processes, not
  premature research, market-data, or order implementations.
- Implement seven cohesive slices. After composition, admission, replay, and
  telemetry can proceed in parallel; final fault qualification joins them.
- GitHub owns live delivery state; this document owns intended scope/sequence.

## Outcome and invariants

Prove recoverable cross-service delivery under duplicates and failure. Preserve
immutable identity/content, private service-owned writes, commit-before-ACK,
publication outside transactions, current claim fencing, bounded retries, and
retained quarantine. Never claim exactly-once transport or successful durable
completion from cancellation or an unknown result.

The conformance executable is test tooling, not a production runtime for the
five product services. Minimal readiness, deadlines, shutdown, permissions, and
operator authentication below are **[Prerequisite from M10]** only. Full
operational readiness remains M10. No M03 data acquisition, M04 calculation,
M07 order semantics, M08 projection platform, or M09 cloud qualification is
pulled into this plan. PostgreSQL server restart/restore stays profile-level
M09/M10 work; M02 tests dependency-connection interruptions.

## Ordered slices

### 1. Typed acceptance contracts and schema compatibility

[Issue #185](https://github.com/stanimirivanov/edge-agent/issues/185) introduces
conformance-only `ApplyProbe`/`ProbeApplied` payloads, exact registry definitions,
derived local schemas, golden and invalid fixtures, frozen v1 baselines, and
the protected T2 `message-schema-contracts` target. Schema lookup performs no
remote retrieval. [ADR-0024](../../decisions/0024-define-synthetic-event-spine-acceptance-contracts.md)
defines compatibility limits and immutable artifact policy. This slice starts no
worker and changes no persistence. **[Prerequisite from M01]** The maintainer
approved global MIT-0 license permission for the schema validator's transitive
dependency and future MIT-0 dependencies; ADR-0024 records the review. The root
allowlist and independent supply-chain sensor must agree on that permission.

### 2. Registry-derived JetStream topology

[Issue #187](https://github.com/stanimirivanov/edge-agent/issues/187) implements
the explicit topology/ACL boundary in the NATS adapter and authenticated isolated
conformance proofs. See the [topology guide](../jetstream-topology.md) and
[ADR-0025](../../decisions/0025-derive-bounded-jetstream-topology-from-the-registry.md).
The broker counts headers in its size limit, so a bounded 1 KiB transport reserve
preserves the portable 256 KiB body ceiling; raw intake still enforces that ceiling.

Derive file-backed command/event streams and durable consumers from the registry.
Enforce retention, payload/pending/storage bounds and minimum subject permissions
**[Prerequisite from M10]**. Separate provisioning authority from runtime roles;
reject incompatible existing topology without deleting evidence. A broker
delivery limit is not durable quarantine. Add protected T3 topology tests to the
existing local-platform profile. Environment/Argo desired state remains owned
by edge-agent-gitops, not application runtime code.

### 3. Multi-process durable command acceptance

[Issue #189](https://github.com/stanimirivanov/edge-agent/issues/189) adds the
finite conformance worker and protected `event-spine-processes` suite. See the
[process guide](../process-acceptance.md) and
[ADR-0026](../../decisions/0026-compose-durable-acceptance-in-independent-processes.md)
for ownership, transaction, lifecycle and evidence limits.

Extend the conformance package with producer, relay, and consumer roles launched
as independent OS processes. Producer acceptance/outbox intent share a commit;
consumer inbox/state/outbound fact share a commit. Use separate owned schemas,
bounded loops, safe readiness/deadlines/shutdown **[Prerequisite from M10]**,
and production adapters. Keep fixture-only crash barriers out of adapter APIs.
Do not introduce an unused generic worker framework or modify product shells.
Register a protected T3 process proof.

### 4. Semantic idempotency, ordering, and replica admission

[Issue #191](https://github.com/stanimirivanov/edge-agent/issues/191) extends the
protected process suite with service-owned semantic receipts, aggregate-local
revision guards, bounded competing replicas, partition progress and connection-loss
recovery. [ADR-0027](../../decisions/0027-enforce-service-owned-semantic-command-ordering.md)
records semantic identity, content comparison and disposable-schema compatibility.

Use service-owned semantic-key/content checks and aggregate revision guards;
the portable inbox continues to deduplicate message identity, not business
policy. Prove identical semantic commands with different IDs produce one
transition, changed content conflicts, missing predecessors defer with bounded
retry, and unrelated partitions progress. Exercise competing processes and
dependency interruption with configured high-water bounds, not flaky throughput
thresholds. `partitionkey` alone is not a FIFO guarantee.

### 5. Authorized recoverable quarantine replay

Compose existing outbound replay audit/release/fence mechanics. For inbound
replay, atomically commit audit authorization and durable dispatch intent, then
publish after commit with restart-safe outcome recording. Distinguish immutable
domain identity from replay-request transport identity and ordinary broker
deduplication. Define original-route/subscriber effects; do not promise exclusive
targeting or repair malformed evidence. Include only the restricted operator
identity/authorization required here **[Prerequisite from M10]**. Add SQLx
metadata, unauthorized/rollback/conflict/restart/receipt-loss proofs and a runbook.

### 6. Exported cross-process event-spine telemetry

Install bounded telemetry exporters in the conformance roots, propagate/link
W3C context, and inspect captured OTLP publication, delivery/handling,
persistence, and acknowledgement results. Maintain payload-safe diagnostics and
bounded labels. Exporter failure cannot change business/storage/settlement
results. Flush and resource lifecycle are **[Prerequisite from M10]**; a full
monitoring/SLO platform is not included. Collector health alone is not evidence
of exported observations.

### 7. Crash, broker-restart, and acknowledgement-loss qualification

Kill/restart real child processes before producer commit, after durable enqueue,
after broker persistence/before outbox completion, and after inbox/domain
commit/before ACK. Include duplicate delivery beyond broker deduplication,
file-backed NATS restart, malformed/poison/unsupported/oversized inputs, and
controlled wire ACK request/confirmation loss. Distinguish lost request,
unknown confirmation, application result loss, and confirmed non-delivery.
One canonical xtask entry point provisions isolated dependencies, runs the
protected acceptance inventory, retains a scenario/result report, and tears
down. Missing prerequisites or required tests fail qualification.

## Sequencing, rollout, and rollback

Slices 1, 2, and 3 merge in order. Slices 4, 5, and 6 may proceed independently
after 3; coordinate shared harness entries and unpublished ADR numbering. Slice
7 follows all three. Each slice must leave the repository buildable and have its
own observable result, issue, verification evidence, and rollback explanation.

These slices add synthetic contracts, topology and process-proof tooling;
existing production contracts need no migration. Every topology or replay change
must document retained-state and mixed-worker compatibility with its ADR, not
silently reinterpret evidence.
Never rewrite frozen baseline schemas to permit a changed published identity.

## Verification and exclusions

Every slice runs `cargo xtask verify`. Extend the existing inventory, discovery
mutations, routes, and local-platform CI; do not create a parallel runner. Report
compilation/discovery separately from executed database/broker/process proofs.
Run metadata freshness when SQL changes and supply-chain checks when dependencies
change. All evidence is synthetic and no model or live execution path exists.

M02 completion requires schemas/compatibility, durable configured topology,
cross-process recovery with one committed transition under adverse delivery,
bounded resource admission, auditable recoverable replay, and exported safe
telemetry. Production service behavior and broad operational/cloud qualification
remain in their owning milestones.
