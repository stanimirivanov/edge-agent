# ADR-0030: Qualify event-spine failure recovery

- Status: Accepted
- Date: 2026-10-18
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Qualify the synthetic event spine using real worker kills, same-store broker
restart and directional wire ACK loss. Keep fault controls in conformance
tooling, use production adapters and retain fail-closed scenario evidence.

## Context

Atomicity, replay, admission and telemetry proofs alone do not establish
recovery when a process dies or a broker confirmation disappears. A timeout
cannot establish whether an external effect occurred.

## Decision

Use finite worker barriers before producer commit, after durable enqueue,
after a real publication receipt/before relay completion, and after inbound
commit/before ACK. Kill/reap real children; fresh restricted workers recover
the stored immutable identity/content. Only the precommit fixture deliberately
holds a transaction for a crash. No production adapter receives a fault API.

Restart the exclusively owned pinned NATS process/container with its original
file store. Refresh Docker's ephemeral port mapping for fresh clients. Verify
retained durable identity, pending work and actual
per-delivery retry metadata, then one committed domain effect. Process death
does not qualify disk/power failure or multi-node replication.

A bounded single-worker loopback frame proxy drops either the actual `+ACK`
request or its exact empty confirmation. Both workers report confirmation
unknown. Independent broker inspection distinguishes pending redelivery from
accepted ACK; committed inbox/state/fact rows remain unchanged. Application
result loss after confirmed publication is a separate crash scenario; explicit
broker size rejection establishes non-acceptance rather than uncertainty.

`cargo xtask m02-qualification` provisions exclusively labelled, loopback-only
PostgreSQL and invokes the existing verification inventory/runner for schemas,
topology and process acceptance. Retained fixed-ID TSV evidence qualifies each
scenario only with its complete passing suite. Setup, census drift, failed or
unavailable checks and failed cleanup fail qualification. CI runs these suites
once outside legacy local-platform regressions and retains reports on failure.
Default acceptance starts no service. Minimal dependency lifecycle is
**[Prerequisite from M10]**.

## Alternatives considered

### Fake port errors or graceful shutdown only

Useful unit tests, but insufficient evidence for process death, retained broker
state or the direction of actual wire loss.

### Restart shared dependencies

Rejected: qualification must exclusively own destructive targets. Existing
explicitly opted-in Compose regression tests remain supported separately.

### A second acceptance runner

Rejected: command and test inventories drift. Provisioning wraps existing
discovery and fail-closed execution, rather than duplicating them.

## Consequences

### Positive

Recovery claims have explicit executable boundaries. Unknown confirmations
never become successful settlement or proof of non-delivery.

### Negative

Qualification requires Docker and cached/downloadable pinned images. Protocol
framing and fixture lifecycle require their own regression coverage.

### Neutral or follow-up

Production/cloud/HA/load qualification, PostgreSQL restart/restore, power-loss
and arbitrary network faults remain M09/M10. Exactly-once transport is not
claimed. No model/provider or live trade operation runs.

## Compatibility and migration

Published contracts, service schemas and production topology do not change.
Disposable fixtures shorten ACK wait and one deduplication window; an actual
later Persisted receipt proves durable inbox independence. Frozen schemas stay
immutable. Rollback removes only disposable tooling/resources, not retained
service evidence or its meaning.

## Security and operations

Workers retain restricted identities and closed finite actions. Proxy frames,
connections and tasks are bounded. Reports exclude credentials, payloads and
source chains. Cleanup uses exact broker receipts or an exclusive per-run
container label. Ordinary errors/panics/deadlines await cleanup; abrupt parent
abort/runner termination still needs disposable-environment teardown.

## Validation

Run `cargo xtask verify` and `cargo xtask m02-qualification`. Protect ignored
identities against missing, renamed, duplicate and extra cases. Unit-test
fragmented/binary framing, bounds, ACK direction, role/action scope, report
failure semantics and cleanup target validation. Compilation/discovery alone
is not executed recovery evidence.
