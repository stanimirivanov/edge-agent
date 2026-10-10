# Messaging adapters

## TL;DR

- `edgeagent-messaging` owns cloud-neutral durable publication, one-at-a-time
  consumption, delivery metadata, and confirmed settlement semantics.
- `edgeagent-messaging-nats` implements those ports with NATS JetStream and
  never leaks broker types into application handlers.
- Every attempt revalidates the envelope, derives its subject from the message
  definition, sends CloudEvents structured JSON, and awaits a persistence acknowledgement.
- The length-prefixed CloudEvents `(source, id)` pair becomes `Nats-Msg-Id`, so
  different producers cannot suppress each other when local IDs collide.
- Pull consumption prefetches one message. Dropping an unsettled delivery allows
  redelivery; settlement is consumed once and waits for broker confirmation.
- Default tests are offline; the local-platform CI job runs publisher and
  consumer conformance tests against the checked-in NATS profile.

## Port ownership

`edgeagent-messaging` keeps a stable public façade in `lib.rs`. `publisher`
owns durable publication receipts and errors; `metadata` owns validated
delivery identities and counters; `consumer` owns one-at-a-time intake and
confirmed settlement. Its private `delivery` module owns the one-shot delivery
and its abandonment guard. `inbox` defines atomic inbound
processing and quarantine, while `outbox` defines relay storage. These are
portable contracts; transport-specific behavior stays in adapter crates.

The outbound storage port requires prevalidated `LeaseDuration` (1 millisecond
through 15 minutes) and `OutboxRetryDelay` (zero through 24 hours). Its retry
value supports immediate eligibility and is distinct from the nonzero broker
`RetryDelay`. Constructors preserve precision; each adapter documents its
persistence precision. See the [PostgreSQL outbox guide](postgres-outbox.md)
for its existing whole-millisecond flooring.

## Publication boundary and control flow

Application code depends on `MessagePublisher`, `PublishReceipt`, and
`PublishErrorKind`. It does not import `async-nats`, name streams, construct
subjects, or interpret broker sequence numbers. The NATS adapter accepts a
client or JetStream context that the composition root already configured with
its endpoint, TLS roots, workload credentials, reconnect policy, and resource limits.

One publish attempt performs these steps:

1. Validate the envelope against its `MessageDefinition`, including type,
   schema, producer authority, partition namespace, and the 256 KiB limit.
2. Derive the exact `edgeagent.command.*` or `edgeagent.event.*` subject.
3. Encode the envelope as CloudEvents structured JSON.
4. Set `Nats-Msg-Id` to the stable, unambiguous CloudEvents `(source, id)` pair.
5. Send through JetStream and wait for its persistence acknowledgement.
6. Return `Persisted` or `Duplicate` without leaking stream names or sequence
   numbers into application policy.

The adapter does not create streams. Deployment configuration must provision
subjects, retention, storage, replicas, limits, and ACLs from the validated
message registry. This separation prevents an application process from
silently changing durability or authorization policy.

## Consumption and settlement boundary

Application handlers depend on `MessageConsumer`, `MessageDelivery`, and
`DeliveryDisposition`. `JetStreamConsumer` accepts an already configured durable
pull consumer, rejects ephemeral or non-explicit-ack configuration, and limits
each broker request to one message. Deployment owns
stream and consumer creation, subject filters, explicit-ack policy, retention,
acknowledgement wait, maximum deliveries, pending limits, replicas, and ACLs.

One delivery follows this control flow:

1. Pull one message without acknowledging it.
2. Validate untrusted JetStream stream identity, positive stream and consumer
   sequences, and delivery count inside the adapter. Assemble portable
   `DeliveryMetadata` from the derived opaque message key, validated subject,
   and positive attempt. Reject raw payloads over the portable 256 KiB limit
   before a delivery is constructed. Invalid text, counters, or payload size
   fail as `Protocol`; pending count and broker sequences do not enter
   application policy.
3. Share an accepted raw structured envelope with the portable delivery
   without copying the NATS payload buffer; the handler still receives an
   untrusted byte slice and validates it independently.
4. Validate the envelope and perform inbox/domain/outbox work in one local
   transaction.
5. After commit, consume the delivery with `Acknowledge` and wait for broker
   confirmation.
6. On a classified transient failure, construct `RetryDelay` before consuming
   the delivery, then use `RetryAfter`. Delays from one millisecond through 24
   hours are accepted; invalid delays never reach broker settlement.
7. Use `Quarantined` only after durable quarantine evidence commits through the
   consumer-owned persistence adapter; it stops JetStream redelivery and is not
   itself a quarantine store.

Settlement consumes `MessageDelivery`, preventing two terminal actions through
the safe API. Validate a retry delay before calling `settle`: a constructor
error leaves the delivery available for a corrected disposition. Dropping an
unsettled delivery emits one structured warning with only attempt and payload
byte count; it sends no acknowledgement and is not durable quarantine. The
delivery owns a settlement directly, so a missing-settlement state cannot be
constructed through the safe API. `settle` transfers a private diagnostic guard
to its future and releases the delivery's payload and metadata. The guard
contains only attempt and byte count, never identity, route, or payload.
Cancelling `receive` may leave an already delivered message unacknowledged,
causing redelivery and an incremented attempt count after ack-wait. Shutdown
policy must not misclassify that as a handler failure. Once `settle` is called,
dropping its unpolled or pending future emits one payload-safe abandonment
warning with `settlement_state = "confirmation_unknown"`. The synchronous
adapter method is invoked when `settle` is called, so an unpolled future does
not universally prove no request occurred. Cancellation does not establish
whether the broker confirmed the action; rely on inbox idempotency if
redelivered. The guard disarms after the adapter returns either success or an
explicit error; errors pass through unchanged and are not abandonment.
`Drop` never acknowledges, retries, or terminally settles a delivery. The
delivery and settlement methods are `must_use`; callers must await and inspect
confirmation. Warnings remain best-effort and cannot cover process abort or
deliberately forgotten values. See
[ADR-0020](../decisions/0020-retain-settlement-abandonment-diagnostics.md).
`Acknowledge`,
delayed negative acknowledgement, and terminal settlement all use JetStream
acknowledgement-sync and complete only after the server confirms receipt.

If broker metadata fails parsing or portable validation, no trustworthy
transport identity exists for handler-owned quarantine. If the raw payload is
over 256 KiB, its exact bytes cannot fit the durable quarantine contract even
when metadata is valid. In either case no `MessageDelivery` exists:
`JetStreamConsumer` returns `Protocol`, leaves the raw message unsettled, and
stops pulling on that instance. The service must fail readiness and surface
the bounded error for operator investigation; it must not loop on the same
instance, acknowledge, or terminally settle the message. Inspect the producer,
stream payload limit, and consumer configuration before replacing the instance
and allowing redelivery. An operator must inspect `max_deliver` and
acknowledgement-wait settings before repeated restart attempts: exhaustion is
not a substitute for durable quarantine. Neither a protocol rejection nor a
retained raw broker message is a committed quarantine record.

## Failure and retry contract

| Category | Meaning | Caller action |
| --- | --- | --- |
| `Contract` | Definition or envelope validation failed before I/O | Do not retry; correct or quarantine the message |
| `Unavailable` | No publish request was accepted or local backpressure prevented it | Retry the identical envelope with bounded backoff |
| `Rejected` | JetStream explicitly rejected the request or no stream owns the derived subject | Remediate configuration/policy before replay |
| `ConfirmationUnknown` | The request was sent but its acknowledgement timed out, disconnected, or could not be decoded | Retry the identical envelope and expect broker deduplication |

Top-level `Display` and `Debug` omit adapter cause text. The wrapped cause
remains available through Rust's error chain for explicit diagnosis; redact it
before structured logging. A receipt proves transport persistence only;
consumer inbox handling still owns exactly-once domain effects.

`PublishError`, `ConsumeError`, `InboxStoreError`, and `OutboxStoreError` offer
`new(kind)` for a classified failure with no underlying cause. It records only
the existing category; `Error::source()` returns `None` and `Debug` reports
`source_present: false`. Do not fabricate an I/O error to satisfy the port.
Use `with_source(kind, cause)` when an adapter or contract has a concrete
failure to preserve. That path keeps its original category and typed cause,
with `source_present: true` and unchanged payload-safe public formatting.
Source presence does not change retry, quarantine, or health policy: callers
continue to match the error kind. Both constructor forms are `must_use`.

JetStream deduplication is bounded by the provisioned stream window. The
transactional PostgreSQL outbox retains the same message identity and immutable
bytes across every retry. Neither mechanism substitutes for consumer inbox
deduplication.

Consumer failures use a separate portable classification:

| Category | Meaning | Caller action |
| --- | --- | --- |
| `Unavailable` | Pull stream could not start, ended, or failed | Reconnect/recreate under bounded service policy; no delivery was acknowledged |
| `Protocol` | Broker delivery metadata was missing, invalid, or outside portable bounds | The instance halts intake without settlement; fail readiness, investigate provisioning/server compatibility, and replace only after correction |
| `InvalidDisposition` | `RetryDelay` construction rejected a delay outside 1 millisecond to 24 hours | Correct handler policy before consuming the delivery; no settlement was sent |
| `ConfirmationUnknown` | Settlement was sent or attempted but confirmation failed | Do not assume success; permit redelivery and rely on inbox idempotency |

Public error formatting remains bounded while adapter causes stay in the error
chain for explicit diagnosis; redact them before logging. A confirmed
acknowledgement advances broker state only;
the committed inbox/domain transaction remains the source of business-effect
idempotency.

## Verification

Normal `cargo test --locked --workspace --all-targets` compiles the adapter and
runs deterministic tests for subject derivation, pre-I/O rejection, failure
classification, and acknowledgement mapping without a broker.
The isolated `delivery_warning` integration test uses controlled manual polls
to exercise unsettled drop, unpolled abandonment, pending cancellation,
resource release, exact disposition forwarding, and explicit success/error
completion. It needs no async runtime or sleep. Keeping all tracing scenarios
in one executable avoids process-global callsite-cache races. Workspace
doctests also reject repeated consumption and ignored settlement results.

After starting the isolated local profile, run the broker conformance test:

```text
EDGEAGENT_NATS_URL=nats://127.0.0.1:4222 cargo test --locked -p edgeagent-messaging-nats --test jetstream_publish -- --ignored --exact persisted_message_identity_deduplicates_on_retry
EDGEAGENT_NATS_URL=nats://127.0.0.1:4222 cargo test --locked -p edgeagent-messaging-nats --test jetstream_consume -- --ignored --exact delivery_settlement_controls_redelivery_and_acknowledgement
EDGEAGENT_NATS_URL=nats://127.0.0.1:4222 cargo test --locked -p edgeagent-messaging-nats --test jetstream_consume -- --ignored --exact abandoned_settlement_future_leaves_delivery_eligible_for_redelivery
EDGEAGENT_NATS_URL=nats://127.0.0.1:4222 cargo test --locked -p edgeagent-messaging-nats --test jetstream_consume -- --ignored --exact oversized_raw_delivery_halts_intake_without_acknowledgement
```

The publisher test verifies broker deduplication. The settlement test provisions
an explicit-ack durable consumer with one pending delivery, verifies delayed
redelivery and incremented attempt metadata, confirms successful acknowledgement,
terminates a simulated durably quarantined message, and confirms no pending
work remains. A separate abandonment regression drops an unpolled
acknowledgement future, requires broker redelivery of the same message, then
awaits an acknowledgement and verifies no pending work remains.
The oversized-delivery test verifies the instance halts without
acknowledging or terminally settling raw bytes. Run them only against an
isolated development or CI broker.

The [event-spine recovery conformance](event-spine-conformance.md) connects these
adapters to the PostgreSQL outbox, relay, inbox, and handler. It proves that an
unacknowledged delivery survives client teardown and reaches one committed
domain transition after reconnection.

## Current limitations

The adapters publish and consume one message at a time. They do not run a
service loop, validate payload schemas, invoke domain handlers, persist inbound
quarantine evidence, extend acknowledgement deadlines, or authorize replay.
Consumer `max_deliver` exhaustion is not a quarantine mechanism and must not be
configured to discard work before the handler policy records a
terminal decision. Stream/consumer provisioning, long-running intake, operator
replay, and messaging telemetry remain separate M02
capabilities. PostgreSQL outbox storage and the bounded relay are documented in
the [outbox](postgres-outbox.md) and [relay](outbox-relay.md) guides. Durable
inbound quarantine storage is documented in the [inbox guide](postgres-inbox.md),
and transactional processing policy in the [handler guide](inbox-handler.md).
