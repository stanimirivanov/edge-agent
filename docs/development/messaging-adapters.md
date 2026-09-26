# Messaging publisher adapters

## TL;DR

- `edgeagent-messaging` owns the cloud-neutral durable publication port and
  retry-relevant outcomes.
- `edgeagent-messaging-nats` implements that port with NATS JetStream and never
  accepts a caller-provided subject.
- Every attempt revalidates the envelope, derives its subject from the message
  definition, sends CloudEvents structured JSON, and awaits a persistence acknowledgement.
- The CloudEvents message ID becomes `Nats-Msg-Id`, so retrying identical bytes
  can use JetStream's bounded duplicate window.
- Default tests are offline; the local-platform CI job runs the ignored broker
  conformance test against the checked-in NATS profile.

## Boundary and control flow

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
4. Set `Nats-Msg-Id` to the stable CloudEvents message ID.
5. Send through JetStream and wait for its persistence acknowledgement.
6. Return `Persisted` or `Duplicate` without leaking stream names or sequence
   numbers into application policy.

The adapter does not create streams. Deployment configuration must provision
subjects, retention, storage, replicas, limits, and ACLs from the validated
message registry. This separation prevents an application process from
silently changing durability or authorization policy.

## Failure and retry contract

| Category | Meaning | Caller action |
| --- | --- | --- |
| `Contract` | Definition or envelope validation failed before I/O | Do not retry; correct or quarantine the message |
| `Unavailable` | No publish request was accepted or local backpressure prevented it | Retry the identical envelope with bounded backoff |
| `Rejected` | JetStream explicitly rejected the request or no stream owns the derived subject | Remediate configuration/policy before replay |
| `ConfirmationUnknown` | The request was sent but its acknowledgement timed out, disconnected, or could not be decoded | Retry the identical envelope and expect broker deduplication |

`Display` exposes only these bounded categories. The wrapped cause remains
available through Rust's error chain for redacted structured diagnostics. A
receipt proves transport persistence only; consumer inbox handling still owns
exactly-once domain effects.

JetStream deduplication is bounded by the provisioned stream window. A later
transactional outbox relay must retain the same message identity and immutable
bytes across every retry and cannot rely on broker deduplication as a substitute
for consumer inbox deduplication.

## Verification

Normal `cargo test --locked --workspace --all-targets` compiles the adapter and
runs deterministic tests for subject derivation, pre-I/O rejection, failure
classification, and acknowledgement mapping without a broker.

After starting the isolated local profile, run the broker conformance test:

```text
EDGEAGENT_NATS_URL=nats://127.0.0.1:4222 cargo test --locked -p edgeagent-messaging-nats --test jetstream_publish -- --ignored --exact persisted_message_identity_deduplicates_on_retry
```

The test provisions an in-memory stream, publishes the same validated envelope
twice, verifies the second acknowledgement is a duplicate, and removes the
stream. Run it only against an isolated development or CI broker.

## Current limitations

This increment publishes one message at a time and relies on the JetStream
context's bounded acknowledgement and in-flight limits. It does not yet define
stream provisioning from the registry, transactional outbox leasing, batch
publication, consumption, inbox deduplication, retries, quarantine, replay, or
messaging telemetry. Those remain separate M02 capabilities.
