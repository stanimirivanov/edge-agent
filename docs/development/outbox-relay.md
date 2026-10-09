# Bounded outbox relay

## TL;DR

- `edgeagent-outbox-relay` is persistence-neutral application policy. It claims
  and resolves at most one record through an `OutboxRelayStore` port per worker
  iteration, bounding each worker to one in-flight publish.
- `edgeagent-outbox-postgres` implements that port with short PostgreSQL
  transactions and lease-guarded state transitions.
- Confirmed broker persistence marks the retained record as published.
- Transient and ambiguous failures use bounded exponential backoff with stable
  identity-derived jitter and preserve the original envelope.
- Contract failures, explicit transport rejection, and exhausted attempts enter
  retained terminal quarantine instead of retrying forever.
- Publication occurs outside database transactions. The claim's owner and
  generation guard the later published, retry, or quarantine transition.

## Control flow

`relay_once` composes the application-owned `MessagePublisher` and
`OutboxRelayStore` ports. It depends on neither transport nor persistence SDKs.
The crate root remains the stable public façade: `coordinator` owns one-record
control flow, `policy` validates worker bounds, `retry` owns deterministic
failure decisions and backoff, `outcome` names durable results, and `error`
preserves bounded failure categories. The public storage port carries the
claim's lease generation through every outcome transition.
`PostgresOutboxRelay` is the PostgreSQL adapter used by a service composition
root:

1. Open a short transaction and claim one eligible record with an expiring lease.
2. Commit the claim before external I/O.
3. Decode the exact stored bytes and validate them against the process registry.
4. Publish the unchanged identity and content outside a database transaction.
5. Open a new short transaction and present the original `ClaimedMessage`
   (which carries its opaque `LeaseGeneration`) with the worker token. Mark the
   record published, release it for retry, or move it into quarantine only
   while that exact claim remains current and unexpired.

One iteration never holds a database transaction during network I/O and never
has more than one publication in flight. A service loop can stop between
iterations for graceful drain. Horizontal throughput comes from additional
workers; PostgreSQL `SKIP LOCKED` gives each worker disjoint records.

If the broker persists a message but the relay cannot record success, the lease
eventually expires and another worker republishes the same `(source, id)` and
canonical bytes. Broker deduplication may recognize the retry, and consumer
inboxes still prevent duplicate committed domain effects after that window.
The storage fence cannot retract a message already sent by a stale worker; it
prevents that worker from changing the state of a newer claim. Reusing the same
worker token on a later claim does not grant the earlier claim authority.

## Retry and terminal-failure policy

`RelayPolicy` requires an inclusive attempt budget from 1 through 100, a base
delay of at least one millisecond, and a maximum delay no greater than 24 hours.
The maximum must not be shorter than the base. Construction also validates the
stable worker token and bounds the lease from one millisecond through 15 minutes;
the storage adapter revalidates both at its trust boundary.

| Publication outcome | Durable relay action |
| --- | --- |
| `Persisted` or `Duplicate` | Mark published and retain the record |
| `Unavailable` below the attempt limit | Release with `transport_unavailable` and a delayed next attempt |
| `ConfirmationUnknown` below the attempt limit | Release with `confirmation_unknown`; retry the identical message |
| `Contract` | Quarantine as `publish_contract` |
| `Rejected` | Quarantine as `transport_rejected` pending configuration remediation |
| Transient or ambiguous failure at the attempt limit | Quarantine with an exhausted-attempt reason |
| Stored bytes fail registry validation | Quarantine as `stored_contract_invalid` before publication |

The delay grows exponentially from the base and is capped before jitter. Stable
FNV-1a hashing of source, ID, and attempt selects a deterministic value from the
upper half of the capped interval. This prevents synchronized retries without a
random dependency and makes policy tests reproducible. The original identity,
envelope, and attempt history never change.

## Quarantine persistence and migration

Services apply every entry in `PostgresOutbox::MIGRATIONS` in order. Migration
0002 adds `quarantined_at` and `quarantine_reason`, enforces paired terminal
state, and excludes quarantined rows from the relay claim index. Migration 0003
adds append-only audit evidence for outbound replay authorization.

Quarantine is retained evidence, not deletion. It clears the active lease and
records a compile-time validated `FailureCode` without exception text or
payload data.
The relay cannot claim the record again. Direct manual table updates are not an
operator replay mechanism because they bypass authorization and audit history.
`PostgresOutbox::replay_quarantined` records the authorization evidence and
releases the original immutable identity and bytes with a reset attempt budget.
Preserved identity makes uncertain earlier publication safe through broker and
consumer deduplication. A future control-plane increment must authenticate and
authorize the operator before invoking that storage boundary.

## Failure and recovery behavior

| Failure point | Recovery behavior |
| --- | --- |
| Claim transaction fails | No publish occurs; restart the iteration under storage policy |
| Worker stops after claim | Lease expiry makes the record eligible again |
| Registry validation fails | Retain and quarantine without transport I/O |
| Publish fails transiently | Persist the classified retry time and bounded code |
| Worker stops after broker persistence | Republish after lease expiry using identical identity and bytes |
| Lease expires or is reclaimed before outcome persistence, including by the same owner token | State update returns `LeaseLost`; the current claim decides the record |
| Outcome transaction fails | Do not report success; lease expiry provides recovery |

`RelayOutcome` exposes idle, published, retry-scheduled, and quarantined states.
The relay emits bounded publication and committed-persistence signals through
the [event-spine telemetry contract](event-spine-telemetry.md). Handled contract
and publication failures retain their error chain for redacted diagnostics.
`RelayError` covers policy, storage availability, and outbox state-transition
failures while preserving internal causes without exposing them in public text.

## Verification

Credential-free unit tests prove configuration bounds, permanent/transient
classification, deterministic jitter, attempt exhaustion, and complete relay
orchestration through in-memory publisher and storage adapters. The fake store
asserts that each published, retry, and quarantine decision forwards the exact
generation from the committed claim. The isolated PostgreSQL conformance test
can be run with:

```text
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-outbox-relay --test postgres_relay -- --ignored --exact relay_bounds_retry_and_quarantines_terminal_failures
```

It verifies confirmed publication, scheduled retry, ambiguous-confirmation
exhaustion, explicit rejection, terminal quarantine, and empty-queue behavior.
Run it only against an isolated development or CI database.
The [PostgreSQL outbox conformance test](postgres-outbox.md#verification) is the
storage-boundary proof that a stale claim cannot resolve a later claim by the
same owner token. Both are required for the lease-fencing contract in
[ADR-0009](../decisions/0009-fence-outbox-transitions-by-claim-generation.md).

## Current limitations

This crate provides one bounded relay iteration, not a continuously running
service. Lifecycle management, a production composition root, readiness,
graceful shutdown, worker-count configuration, telemetry exporter installation,
active lease extension, operator inspection and replay APIs, and quarantine
retention remain separate capabilities. The relay is sequential per worker by
design; higher per-worker concurrency requires an explicit lease-duration and
backpressure design rather than unbounded tasks.
