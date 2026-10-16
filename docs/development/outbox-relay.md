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
control flow, `observation` owns validated diagnostic context and explicit stage
timers, and `resolution` records published, retry, and quarantine outcomes.
`policy` validates worker bounds, `retry` owns deterministic
failure decisions and backoff, `outcome` names durable results, and `error`
preserves bounded failure categories. The public storage port carries the
claim's lease generation through every outcome transition.
`PostgresOutboxRelay` is the PostgreSQL adapter used by a service composition
root:

1. Open a short transaction and claim one eligible record with an expiring lease.
2. Commit the claim before external I/O.
3. Decode the exact stored bytes and validate them against the process registry.
4. If the acquired claim exceeds the current attempt budget, quarantine without
   invoking the publisher. Otherwise publish the unchanged identity and content
   outside a database transaction, including on the final permitted claim.
5. Open a new short transaction and present the original `ClaimedMessage`
   (which carries its opaque `LeaseGeneration`) with the worker token. Mark the
   record published, release it for retry, or move it into quarantine only
   while that exact claim remains current and unexpired.

One iteration never holds a database transaction during network I/O and never
has more than one publication in flight. A service loop can stop between
iterations for graceful drain. Horizontal throughput comes from additional
workers; PostgreSQL `SKIP LOCKED` gives each worker disjoint records.
This does not promise FIFO, partition ordering, ordered returned batches, or
completion order across workers.

If the broker persists a message but the relay cannot record success, the lease
eventually expires and another worker, if still within budget, republishes the same `(source, id)` and
canonical bytes. Broker deduplication may recognize the retry, and consumer
inboxes still prevent duplicate committed domain effects after that window.
The storage fence cannot retract a message already sent by a stale worker; it
prevents that worker from changing the state of a newer claim. Reusing the same
worker token on a later claim does not grant the earlier claim authority.

## Retry and terminal-failure policy

`RelayPolicy` requires an inclusive attempt budget from 1 through 100, a base
delay of at least one millisecond, and a maximum delay no greater than 24 hours.
The maximum must not be shorter than the base. Construction also validates the
stable worker token and constructs a portable `LeaseDuration` from one
millisecond through 15 minutes. The policy and calculated retry delay use the
portable `OutboxRetryDelay` bound before storage calls; the storage adapter
continues to validate the worker token. Outbound storage supports immediate
retry, but relay policy deliberately requires nonzero backoff. The timing
ownership and compatibility decision is recorded in
[ADR-0019](../decisions/0019-validate-outbox-timing-at-the-port.md).

| Publication outcome | Durable relay action |
| --- | --- |
| `Persisted` or `Duplicate` | Mark published and retain the record |
| `Unavailable` below the attempt limit | Release with `transport_unavailable` and a delayed next attempt |
| `ConfirmationUnknown` below the attempt limit | Release with `confirmation_unknown`; retry the identical message |
| `Contract` | Quarantine as `publish_contract` |
| `Rejected` | Quarantine as `transport_rejected` pending configuration remediation |
| Transient or ambiguous failure at the attempt limit | Quarantine with an exhausted-attempt reason |
| Stored bytes fail registry validation | Quarantine as `stored_contract_invalid` before publication |
| Valid acquired claim exceeds the current attempt limit | Quarantine as `publication_budget_exhausted` without invoking the publisher |

Attempts count acquired claims, not physical network requests. Cancellation
does not refund them, even when publication never starts. A recovered claim
equal to the limit may publish; one above it must not. Reducing policy does
not reset history. Exhausted recovery can quarantine an already persisted
broker message whose result was lost; quarantine does not prove non-delivery.
Reconcile effects or use audited operator replay, preserving identity/content.
[ADR-0022](../decisions/0022-bound-relay-publication-by-acquired-claims.md)
records this availability/resource tradeoff. Returned failures on the final
allowed publication retain their existing classified exhaustion reasons.

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
| Worker stops after broker persistence | Recover after expiry with identical identity and bytes; republish only within budget, otherwise quarantine for reconciliation |
| Lease expires or is reclaimed before outcome persistence, including by the same owner token | State update returns `LeaseLost`; the current claim decides the record |
| Outcome transaction fails | Do not report success; lease expiry provides recovery |

`RelayOutcome` exposes idle, published, retry-scheduled, and quarantined states.
The relay emits bounded publication and committed-persistence signals through
the [event-spine telemetry contract](event-spine-telemetry.md). Handled contract
and publication failures retain their error chain for redacted diagnostics.
Skipped publication has a distinct policy failure with no invented publisher
cause and no publication event; only quarantine persistence is observed.
`RelayError` covers policy, storage availability, and outbox state-transition
failures while preserving internal causes without exposing them in public text.

Stored source, ID, type, subject, and bytes remain untrusted until the complete
record passes envelope, registry, and denormalized-column validation. Rejected
records emit only bounded stage/outcome tokens, attempt, and duration: every
identity and correlation field stays empty, including when quarantine fails.
The relay neither truncates rejected identities nor copies them into a fallback
context. Valid records obtain correlation fields from the validated envelope.
Registry metadata lookup does not repeat envelope validation.

Observation occurs only after an explicit returned result, never from a
destructor. The coordinator shows claim, validation, publication, and the chosen
resolution in order. Resolution forwards the original claim and owner, attempts
one durable transition, records its actual result, and propagates failure.
Neither observation nor resolution starts compensation or an alternate
transition after cancellation or a storage error.

## Verification

The protected T2 `relay-observation-contracts` check runs one isolated tracing
executable. It asserts emitted fields, exact operation order, all publication
classifications, returned store failures, original claim/owner forwarding, and
the composed relay future's `Send` contract. Discovery rejects a missing,
renamed, duplicated, or unregistered test. This is an application and telemetry
proof, not a database or broker proof:

```text
cargo xtask verify --check relay-observation-contracts
```

Credential-free unit tests prove configuration bounds, permanent/transient
classification, deterministic jitter, attempt exhaustion, and complete relay
orchestration through in-memory publisher and storage adapters. The fake store
asserts that each published, retry, and quarantine decision forwards the exact
generation from the committed claim, the configured typed lease, and the exact
calculated typed retry delay. Bound-edge policies and capped jitter receive
separate regression coverage. The isolated PostgreSQL conformance test
can be run with:

```text
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-outbox-relay --test postgres_relay -- --ignored --exact relay_bounds_retry_and_quarantines_terminal_failures
```

It verifies confirmed publication, scheduled retry, ambiguous-confirmation
exhaustion, explicit rejection, terminal quarantine, and empty-queue behavior.
Run it only against an isolated development or CI database.
Its shared owned-schema fixture bounds connection, case, and cleanup work and
awaits cleanup even after errors, caught panics, and controlled timeouts. The
initial migration/enqueue/upgrade scenario remains part of this proof. See
[outbox recovery verification](postgres-outbox.md#competing-workers-cancellation-and-fixture-recovery)
for the protected fixture and competing-connection/cancellation suite; CI Compose
teardown remains the process-crash backstop.
The [PostgreSQL outbox conformance test](postgres-outbox.md#verification) is the
storage-boundary proof that a stale claim cannot resolve a later claim by the
same owner token. Both are required for the lease-fencing contract in
[ADR-0009](../decisions/0009-fence-outbox-transitions-by-claim-generation.md).

`cargo xtask verify --check relay-recovery-budget` cancels the actual relay
while a controlled publisher is pending against real PostgreSQL. It asserts
the durable lease, same-owner recovery with a new generation, over-budget
quarantine, unchanged bytes, no second publisher invocation, and stale-fence
rejection. `cargo xtask verify --check event-spine-producer` adds the real
JetStream held-confirmation proof described in the
[event-spine conformance guide](event-spine-conformance.md#producer-result-loss-and-the-recovery-budget).
Both join the existing local-platform profile; discovery alone is not execution.

## Current limitations

The [refactoring plan](plans/outbox-relay-refactor.md) sequences telemetry
hardening, recovery-aware attempt budgets, and explicit configuration/runtime
failure contracts. The first slice belongs to issue #174; the second belongs to
[issue #176](https://github.com/stanimirivanov/edge-agent/issues/176).

This crate provides one bounded relay iteration, not a continuously running
service. Lifecycle management, a production composition root, readiness,
graceful shutdown, worker-count configuration, telemetry exporter installation,
active lease extension, operator inspection and replay APIs, and quarantine
retention remain separate capabilities. The relay is sequential per worker by
design; higher per-worker concurrency requires an explicit lease-duration and
backpressure design rather than unbounded tasks.
