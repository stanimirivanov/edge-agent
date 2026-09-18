# ADR-0002: Use portable at-least-once event messaging

- Status: Proposed
- Date: 2026-09-18
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use CloudEvents 1.0 metadata with versioned EdgeAgent command/event payloads.
- Use NATS JetStream as the local and portable durable broker; permit cloud-native adapters that pass the same conformance suite.
- Assume at-least-once delivery, duplicates, delay, and out-of-order messages.
- Combine transactional outbox, consumer inbox/deduplication, idempotent handlers, bounded retries, and quarantine/dead-letter handling.
- Guarantee ordering only within an explicitly partitioned aggregate stream.
- Do not claim transport-level exactly-once business effects.

## Context

Multiple deployables need to coordinate research requests, evidence snapshots,
artifact publication, dry-run order execution, lifecycle updates, and audit
projection. Synchronous request chains would couple availability and make replay
or independent scaling difficult. Cloud event routers, durable queues, and
streams also expose materially different delivery, ordering, and replay semantics.

CloudEvents provides portable event metadata and Rust SDK support without
defining the payload or transport. It is a CNCF Graduated specification used by
major cloud event platforms; see the [CloudEvents project](https://cloudevents.io/).

NATS JetStream persists and replays messages and explicitly provides
at-least-once delivery through acknowledgment and redelivery. EdgeAgent must
therefore make idempotency an application property rather than assume a broker
can provide exactly-once business effects. See the
[JetStream concepts](https://docs.nats.io/concepts/jetstream).

## Decision

### Envelope and payload

Commands and events use a CloudEvents 1.0-compatible envelope containing:

- `specversion`, `id`, `source`, `type`, `subject`, `time`, and `datacontenttype`;
- `dataschema` identifying the immutable payload schema;
- `correlationid` linking one end-to-end workflow;
- `causationid` identifying the command/event that caused this message;
- `traceparent` and optional `tracestate` for telemetry propagation;
- `partitionkey` for the aggregate whose ordering matters; and
- payload bytes validated against the declared schema.

Message identity and trace identity are separate. Replaying an event preserves
its event ID but starts or links an operational trace according to replay policy.

Commands express intent and use imperative type names such as
`com.edgeagent.execution.submit-dry-run-order.v1`. Events describe completed
facts in past tense, such as `com.edgeagent.execution.dry-run-order-accepted.v1`.
The schema version is part of the type and payload metadata; field meaning never
changes silently within a version.

### Delivery semantics

- Delivery is at least once. Consumers expect duplicates.
- Ordering is guaranteed only per documented partition key and only when the
  selected transport/profile supports the required ordered consumer.
- A producer persists domain state and an outbox record in one local transaction.
- A relay publishes the outbox record and records confirmation without deleting
  evidence needed for recovery.
- A consumer validates envelope and schema, checks its inbox/deduplication key,
  performs its local state transition and inbox write atomically, then acknowledges.
- Handlers compare immutable command content when reusing an idempotency key;
  the same key with different content is a conflict, not a retry.
- Transient failures use bounded exponential backoff with jitter. Permanent
  schema, authorization, invariant, and policy failures are quarantined without blind retry.
- Poison messages retain redacted diagnostics and operator recovery actions.
- Consumers reject unknown major versions and unsupported event types safely.

### Broker profiles

NATS JetStream is the required local and portable Kubernetes broker. The first
implementation uses `async-nats` behind an application-owned messaging port.

Managed profiles may substitute:

- Google Cloud Pub/Sub for durable messaging and Eventarc for cloud-resource routing;
- Azure Service Bus for commands/work queues, Event Hubs for retained ordered
  streams, and Event Grid for cloud-resource routing;
- AWS SQS/SNS for work distribution, Kinesis for retained ordered streams, and
  EventBridge for cloud-resource routing.

These are capability mappings, not transparent drop-in replacements. Each
profile declares delivery, ordering, retention, replay, payload limits, dead
lettering, scheduling, identity, and cost behavior and must pass contract and failure tests.

## Alternatives considered

### Synchronous HTTP between every service

- Benefits: direct control flow and simple local debugging.
- Costs and risks: temporal coupling, cascading failures, poor replay, and no
  durable handoff for long-running research/execution workflows.
- Reason not selected: HTTP remains appropriate for queries and bounded request/
  response, but not as the only coordination mechanism.

### Kafka as the universal broker

- Benefits: mature durable log, partition ordering, replay, and ecosystem.
- Costs and risks: heavier local and portfolio deployment footprint; additional
  operational complexity for modest initial throughput.
- Reason not selected: JetStream provides the required durable messaging and
  replay semantics with a smaller local footprint. Kafka-compatible profiles
  may be added when streaming scale or ecosystem integration requires them.

### Cloud-native broker APIs in domain/application code

- Benefits: direct access to every provider-specific feature.
- Costs and risks: locks contracts and failure semantics to one cloud and makes
  local conformance difficult.
- Reason not selected: cloud adapters can expose selected capabilities without
  leaking vendor SDK types into application policy.

### Exactly-once delivery claims

- Benefits: superficially simpler consumer reasoning.
- Costs and risks: transport acknowledgments cannot make arbitrary external
  side effects globally atomic, especially across clouds and databases.
- Reason not selected: at-least-once plus idempotent local effects is explicit,
  testable, and portable.

## Consequences

### Positive

- Workflows survive transient service outages and support replay.
- Services can scale and deploy independently behind stable schemas.
- Local NATS and managed-cloud profiles exercise the same application contracts.
- Audit and portfolio projections can rebuild from immutable events where retention permits.

### Negative

- Every consumer needs inbox, retry, quarantine, and observability behavior.
- Eventual consistency affects APIs, tests, and user-visible status.
- Schema evolution and multi-version rollout become ongoing engineering work.
- Managed brokers differ enough that conformance requires deliberate feature constraints.

### Neutral or follow-up

- Queries may use HTTP/gRPC or read models when synchronous freshness is required.
- Large evidence payloads live in object storage; messages carry an immutable URI,
  integrity hash, size, media type, and entitlement metadata.
- A workflow engine may later coordinate complex sagas, but is not selected here.

## Compatibility and migration

The contracts workspace stores schemas and golden fixtures by major version.
Producers add optional fields before consumers require them. Breaking changes
publish a new type/version, deploy tolerant consumers first, then producers,
and retire the old version only after retention and replay windows close.

Broker migration uses dual-publish only through a controlled bridge that
preserves event identity and records the original transport metadata. Consumers
must not process the same event twice across old and new paths.

## Security and operations

- Services authenticate with workload identity and authorize publish/subscribe subjects by least privilege.
- Payloads are encrypted in transit and at rest according to the deployment profile.
- Sensitive or licensed data is referenced, not copied into broadly consumed events.
- Event metadata and errors are bounded to prevent log or metric cardinality attacks.
- Retention, replay, and deletion policy is defined per stream and data class.
- Distributed traces propagate W3C context, while logs preserve event, causation, correlation, and consumer-attempt IDs.

## Validation

- Contract fixtures round-trip across serialization and schema validation.
- Conformance tests inject duplicate, delayed, out-of-order, unknown-version,
  oversized, unauthorized, poison, and redelivered messages.
- Crash tests stop consumers before and after local commit and before acknowledgment.
- Replay produces identical deterministic state while preventing duplicate effects.
- Each deployment profile publishes a capability report and passes the same required semantics.
