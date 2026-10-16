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
- Each pull request asks for one message; a completed or cancelled receive
  retains no activated pull worker. Dropping an unsettled delivery allows
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
pull consumer, rejects ephemeral, non-explicit-ack, or header-only configuration, and limits
each broker request to one message. Deployment owns
stream and consumer creation, subject filters, explicit-ack policy, retention,
acknowledgement wait, maximum deliveries, pending limits, replicas, and ACLs.
Admission checks the supplied SDK handle's cached configuration. Provisioning
must not change it under an active adapter: quiesce intake, apply the change,
retrieve an updated consumer handle, and recreate the adapter. Header-only
delivery strips valid envelope bodies and cannot enter application quarantine
as if those bodies were malformed.

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
acknowledgement-sync through the SDK's Core NATS request/reply transport. The
adapter preserves the configured JetStream timeout and accepts only an empty
reply with no status or an explicit `200` status. No responders, timeouts,
non-success status, and nonempty replies are `ConfirmationUnknown`, not
successful settlement. Invalid-reply causes contain only numeric status and
byte count, never response bodies or descriptions. The pinned SDK's
`double_ack_with` treats any received reply as success; the adapter deliberately
does not use that unchecked confirmation path.

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

The adapter's crate-root facade re-exports the same two public types. Private
`publisher` and `consumer` modules own their distinct lifecycles; consumer
`intake`, `metadata`, and `settlement` modules own state, pure broker validation,
and confirmation respectively. Adjacent test modules exercise each owner.
Consumer `Debug` exposes lifecycle state only, never client configuration,
credentials, routes, or buffered payloads.

Intake owns an explicit active, protocol-halted, or closed state. Active intake
retains its configured consumer and an unstarted initial stream. A polled
receive takes ownership of that stream, or creates a fresh one. Completion,
failure, and cancellation drop the activated stream and abort its renewal task
before delivery handoff, even while the adapter stays alive. Stream exhaustion or SDK
`ConsumerDeleted`/`PushBasedConsumer` errors also release it and close intake.
The first terminal SDK error retains its typed cause as `Unavailable`; later
closed calls return `Unavailable` immediately without I/O. Halted calls return
`Protocol` immediately. Missing heartbeat, pull failure, no responder, and
other nonterminal SDK errors preserve active intake and their typed causes.
Cancellation preserves logical intake ownership, not the old subscription.
The next receive creates a fresh stream. A previously issued request may already
have delivered work to the abandoned subscription, so recovery can wait for
ack-wait and increment the attempt count. No destructor performs a disposition.
Receipt, stream release, validation, and delivery construction have no
intervening await.

One message per request is not a consumer-global pending-acknowledgement limit.
A caller may invoke receive again while retaining an earlier delivery; replicas
and reconnects can also leave broker-side work outstanding. Deployment must
configure compatible pending, retry, and acknowledgement budgets. This adapter
does not claim a global one-message cap or retract uncertain in-flight pulls.

Aborting the worker does not retract already-issued or queued NATS requests,
clear broker-side pending work, or perform a disposition. In-flight work may
remain unacknowledged. A halted instance cannot reopen itself; investigate
and replace it under the policy above. Recover a deleted/reconfigured durable
consumer through provisioning before constructing a replacement adapter.

## Stored-envelope validation boundary

An outbox claim is a persistence record, not a validated publication. Its
constructor accepts stored text and bytes so a corrupt or obsolete record can
still reach the relay's quarantine path. `envelope_bytes()` returns that exact
untrusted evidence; it does not authorize publication.

`ClaimedMessage::validated_envelope` applies these gates in order:

1. Reject raw input over 256 KiB before JSON or CloudEvents parsing. Valid input
   at the boundary, including insignificant whitespace, remains eligible.
2. Decode and validate the envelope's required metadata and JSON payload shape.
3. Resolve the exact major-version type and enforce registered schema,
   partition namespace, event-producer authority, and encoded-size policy.
4. Compare all four stored columns (`id`, `source`, `type`, and
   `transport_subject`) with the decoded identity and registry-derived route.

Revalidation never rewrites stored bytes, attempts, or lease generations.
Malformed, invalid-metadata, and raw oversize failures retain a typed, payload-
safe `MessageContractError` inside `MessageRoutingError::Envelope`. Unsupported
types and mismatches retain their routing category or fixed field name, not
the actual values. The raw-size error is distinct from the registry's encoded-
size error. This is envelope/routing validation, not domain payload schema
validation.

The relay quarantines rejected records with `stored_contract_invalid` under
the original claim fence and never invokes publication. A failed quarantine
remains a storage error; it must not report a successful terminal outcome or
choose a replacement transition. This ordering is an application contract;
adapters still own atomic, lease-fenced persistence.

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

Publication failure translation remains phase-aware and exhaustive over SDK
error kinds. During the acknowledgement phase, SDK `Other` wraps both unreadable
responses and typed server errors. Typed `STREAM_MESSAGE_EXCEEDS_MAXIMUM`
(10054) proves a pre-persistence rejection and maps to `Rejected`; undecodable
responses, generic storage failures, and unclassified server codes remain
`ConfirmationUnknown`. Extend that allowlist only with a typed code and proof
that it establishes rejection before persistence. Never classify by matching a
server's description string. Local send-side `MaxAckPending` is backpressure
before acceptance and remains `Unavailable`.

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

## Cancellation and recovery protocols

The crate-level rustdoc maps ownership and canonical flows. Every side-effecting
port method documents cancellation separately from an explicit error. Dropping
a future means the caller observed no result; it is not an `Unavailable`
classification or proof of rollback. Adapters may perform synchronous work when
constructing a future, so even an unpolled future is not universally effect-free.
No destructor schedules a replacement publication, storage transition, or
broker disposition.

| Interrupted operation | Possible committed state | Recovery |
| --- | --- | --- |
| `publish` | Broker persisted the message without an observed receipt | Retry the same identity and immutable bytes under bounded policy; retain consumer inbox deduplication beyond the broker window |
| `process` | Inbox, domain work, and transactional outbox committed atomically | Leave delivery unsettled; redelivery returns `Duplicate` if committed, otherwise applies once |
| Inbound `quarantine` | Exact evidence retained without an observed disposition | Leave delivery unsettled; retry identical evidence until `Inserted` or `AlreadyPresent` confirms commit before terminal settlement |
| `claim_one` | Lease committed without a returned claim | Do not publish an unknown claim; expiry permits reacquisition with a higher generation, subject to the relay's acquired-claim budget |
| `mark_published` | Published marker committed, or lease still active despite broker persistence | Inspect committed state or recover through a later valid claim; never infer an alternative outcome |
| `release_for_retry` | Lease release and next-eligibility delay committed | Respect committed eligibility and acquire a new generation; do not bypass the delay |
| Outbound `quarantine` | Terminal quarantine committed | Inspect state; committed quarantine requires separately authorized replay, not automatic retry |
| Broker settlement | Broker may have applied the disposition without confirmation | Do not choose a replacement disposition; rely on redelivery and retained inbox/quarantine identity |

Processing identity is `(consumer_name, source, id)`, with exact canonical
envelope content. Quarantine identity is `(consumer_name, delivery_key)` because
poison bytes may lack valid CloudEvents identity. `AlreadyPresent` requires the
same transport subject, exact payload bytes, and failure code. Attempts are
observations: retain the first observation and monotonically increase the last
attempt. A changed attempt alone is not conflicting evidence. Changed immutable
quarantine evidence fails closed as portable `Invariant`, whereas changed
processed envelope content uses `MessageIdentityConflict`. Neither result
overwrites evidence or permits terminal settlement. Rust equality of evidence
values is not the durable idempotency comparison.

The [inbox coordinator](inbox-handler.md) and [outbox relay](outbox-relay.md)
own recovery ordering; adapters own atomic persistence and transport behavior.
The relay's startup `RelayPolicyError` is distinct from runtime `Unavailable`,
`StateTransition`, and `Invariant` categories. Failed retry/quarantine keeps
the actual storage error in `Error::source()` and the earlier message failure
in `triggering_failure()`. A rejected transition need not mean lease loss.
Neither cause is rendered publicly; explicitly traversed adapter causes still
need redaction. A confirmed publication is not fabricated as a triggering error.
The relay's inclusive budget counts committed claims, including abandoned
ones, not network requests. Over-budget recovery quarantines without invoking
publication and does not fabricate a transport error. Earlier effects may
already exist: quarantine is not evidence of non-delivery. See
[ADR-0022](../decisions/0022-bound-relay-publication-by-acquired-claims.md).
Graceful shutdown stops intake and gives owned work a bounded completion window.
If it must cancel, it preserves the identities and durable state above rather
than converting shutdown into a handler failure. This contract does not add a
worker loop, lease extension, compensation service, or shutdown runtime.

## Verification

Normal `cargo test --locked --workspace --all-targets` compiles the adapter and
runs deterministic tests for subject derivation, pre-I/O rejection, failure
classification, and acknowledgement mapping without a broker.
The messaging `claimed_envelope` integration executable directly tests valid
commands and authoritative events, inclusive raw-size boundaries, malformed or
invalid decoded input, unsupported versions, registry policy, every stored
column comparison, and payload-safe error chains. The `auto_traits` executable
checks sendable deliveries and future aliases with borrowed lifetimes, and
sendable, shareable, owned errors without requiring deliveries or settlement
adapters to be `Sync`. Relay unit tests prove rejection prevents even an
unpolled publication invocation and retains the original fence and typed
failure through quarantine, including a failed quarantine:

```text
cargo test --locked -p edgeagent-messaging --test claimed_envelope --test auto_traits
cargo test --locked -p edgeagent-outbox-relay --lib stored_
```

The existing Linux/Windows workspace gate runs these tests without a broker,
database, runtime installation, or new dependencies.
The isolated `delivery_warning` integration test uses controlled manual polls
to exercise unsettled drop, unpolled abandonment, pending cancellation,
resource release, exact disposition forwarding, and explicit success/error
completion. It needs no async runtime or sleep. Keeping all tracing scenarios
in one executable avoids process-global callsite-cache races. Workspace
doctests also reject repeated consumption and ignored settlement results.

The handler and relay each have a deterministic `cancellation` integration
executable. Manual single polls stop at modeled pre-commit and post-commit,
pre-result boundaries. The handler proves no premature settlement or invented
failure resolution, one domain transition on redelivery, and exact quarantine
identity/attempt handling. The relay proves abandoned-claim expiry, immutable
publication retry with duplicate receipts, and no replacement outcome around
all three completion operations. An explicit clock controls eligibility; tests
use no runtime, sleeps, broker, or database. They prove application ordering
against fakes, not SQLx cancellation mechanics or real broker durability:

```text
cargo test --locked -p edgeagent-inbox-handler --test cancellation
cargo test --locked -p edgeagent-outbox-relay --test cancellation
cargo test --locked -p edgeagent-messaging --doc
```

The existing Linux/Windows workspace gate runs both executables and the
commit-before-terminal-settlement doctest. PostgreSQL conformance separately
proves inbox atomicity, exact quarantine equality, monotonic attempts, and
same-owner lease fencing; JetStream conformance proves real settlement and
redelivery. A passing fake does not qualify a new adapter.

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

Five additional broker-boundary regressions are protected by the canonical
runner and execute in the existing local-platform CI job:

```text
EDGEAGENT_NATS_URL=nats://127.0.0.1:4222 cargo xtask verify --check nats-no-stream --check nats-size-rejection --check nats-header-only --check nats-publish-oversize --check nats-settlement-response
```

These prove real no-stream rejection, typed stream-size rejection with no stored
message, header-only rejection before intake, and oversize publication without
transport traffic. The response proof deletes real consumers after receipt and
requires all three dispositions to report actual no-responder errors, not
confirmation. T2 response-validation tests independently reject nonempty bodies
and non-success status without retaining private reply text. Existing real-stream
tests own positive settlement/redelivery.
All NATS suites share process-scoped owned resource naming and bounded cleanup
after success, ordinary failure, synchronous/asynchronous panic, and timeout.
No fixture pre-deletes a foreign resource. Distinct publication/consumption
subjects avoid overlap between suites. Use an exclusive development broker;
these fixed synthetic subjects do not authorize concurrent independent runs
against a shared account. Disposable CI teardown covers task cancellation and
process abort, which cannot await fixture cleanup.

Two lifecycle proofs also run through the canonical local-platform inventory:

```text
EDGEAGENT_NATS_URL=nats://127.0.0.1:4222 cargo xtask verify --check nats-halted-worker --check nats-closed-intake
```

The halted-worker proof keeps the poisoned adapter alive beyond the pinned
SDK's 30-second expiry plus five-second renewal margin, observing actual pull
subjects rather than only the public gate. It then forces that client's
reconnection; an active consumer on the same connection provides a positive
pull witness while the halted worker remains stopped. Poison bytes stay in
the stream with one pending acknowledgement. This timer-specific proof takes
at least 38 seconds, requires an exclusive broker, and does not establish
a sustained prefetch bound or broker-server restart durability.
The closure proof cancels a polled receive and recovers a delivery, then waits
for another registered pull before deleting the real durable consumer. It
verifies the typed terminal error, requires repeated calls to resolve without
polling, and observes no renewal pull for another 37 seconds while the closed
handle remains alive. T2 ownership probes separately prove
immediate, one-time stream release, all six SDK error classifications, and
cancellation-safe intake and receive-worker release; they do not substitute for
the broker proofs.

Two additional checks protect receive-scoped ownership and the shared fixture:

```text
EDGEAGENT_NATS_URL=nats://127.0.0.1:4222 cargo xtask verify --check nats-intake-bound --check nats-fixture-cleanup
```

The intake proof queues four messages with broker `max_ack_pending = 8` and
`max_batch = 8`, verifies the actual request's batch and expiry, holds the first
unsettled delivery past the SDK renewal deadline, and observes no further pull.
Broker state still has one delivered/pending acknowledgement and three unread
messages. Subsequent explicit receives drain them in order. The previous
retained-stream implementation fails this test at its 35-second renewal.
This proves idle receive behavior, not a hard SDK-buffer bound across network
recovery or a consumer-global concurrency limit. The cleanup proof forces all
four failure modes and requires typed `STREAM_NOT_FOUND` lookups afterward;
a transport failure cannot masquerade as successful deletion.
Offline tests preserve crate-root port/auto-trait contracts and payload-safe
consumer state formatting. The existing census and local-platform job own
all thirteen NATS checks; no parallel verification implementation is added.

The ordered [NATS adapter refactor plan](plans/messaging-nats-refactor.md) keeps
confirmation correctness, consumer lifecycle, and capability/fixture extraction
in independently reviewable slices.

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
