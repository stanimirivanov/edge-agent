# EdgeAgent message envelope contract

## TL;DR

- Commands and events use one validated CloudEvents 1.0 structured JSON
  envelope owned by `edgeagent-contracts`.
- Producers supply all identifiers, occurrence time, schema identity, partition,
  and trace context; the contract performs no random or wall-clock work.
- Required EdgeAgent extensions are `correlationid`, `causationid`,
  `idempotencykey`, `partitionkey`, and `traceparent`; `tracestate` is optional.
- Invalid versions, names, identifiers, trace context, attributes, or non-JSON
  payloads fail before a message reaches a transport or handler.
- A validated registry owns subjects, command handlers, event producers,
  partition namespaces, portable size limits, delivery mode, and retention.
- Generated payload schemas, NATS adapters, outbox/inbox persistence, retries,
  and quarantine remain separate M02 capabilities.

## Purpose and boundary

The envelope is the portable boundary between domain payloads and message
transports. It standardizes identity, compatibility, ordering scope,
idempotency, observability, and schema discovery without importing NATS, a
cloud SDK, a database, or a service implementation into contract code.

`MessageMetadata` describes caller-owned metadata. `MessageEnvelope` creates a
validated CloudEvent from any Serde-serializable payload, encodes structured
JSON, decodes untrusted bytes, revalidates every required invariant, and
deserializes the data field into a caller-selected payload type.

The implementation uses the official CloudEvents Rust SDK with all transport
features disabled. Protocol adapters will consume the validated structured
bytes rather than redefine the envelope.

`MessageDefinition` binds one exact major-version type to its schema, semantic
kind, owner, partition namespace, and retention class. `MessageRegistry`
rejects invalid or duplicate definitions and resolves untrusted envelopes only
by their exact type. This makes broker configuration and service authorization
derivable from code-reviewed contracts rather than duplicated strings.

## Wire contract

Every message contains:

| Attribute | Requirement |
| --- | --- |
| `specversion` | Exactly CloudEvents `1.0` |
| `id` | Non-empty bounded EdgeAgent identifier, unique within `source` |
| `source` | Bounded absolute URI identifying the producer |
| `type` | `com.edgeagent.<domain>.<name>.v<major>` |
| `subject` | Bounded visible aggregate-relative subject |
| `time` | RFC 3339 occurrence or command-creation time |
| `datacontenttype` | Exactly `application/json` |
| `dataschema` | Absolute URI for the immutable payload schema |
| `correlationid` | Stable end-to-end workflow identifier |
| `causationid` | Identifier of the directly causal message |
| `idempotencykey` | Stable retry key whose content must remain identical |
| `partitionkey` | Aggregate key within which ordering matters |
| `traceparent` | Non-zero W3C Trace Context version 00 value |
| `tracestate` | Optional bounded visible W3C vendor state |
| `data` | JSON payload; its meaning is owned by the declared schema |

Identifiers are limited to 128 bytes and use ASCII letters, digits, hyphen,
underscore, dot, or colon. Other bounded metadata is limited to visible ASCII
without spaces so it remains safe in logs, broker headers, metrics, and
cross-cloud adapters. Payload content is not copied into validation errors.

The committed
[`research-request-received.json`](../../crates/contracts/fixtures/v1/research-request-received.json)
fixture is the golden structured representation. It is synthetic and contains
no market data, credential, or private identifier.

## Routing and ownership

Subjects are derived, never hand-written:

```text
edgeagent.command.<domain>.<name>.v<major>
edgeagent.event.<domain>.<name>.v<major>
```

Commands use work-queue delivery. Their `owner` is the sole component allowed
to handle the command; multiple replicas may join that component's durable
consumer group, but only one member handles a delivery attempt. Command
publishers are authorized separately because more than one trusted component
may request the same capability.

Events use retained-stream delivery. Their `owner` is the sole authoritative
producer, and the envelope `source` must equal that component's stable source
URI. Multiple independently checkpointed consumers may subscribe and replay.
No event consumer becomes authoritative for the fact by projecting it.

Subjects do not contain aggregate identifiers. `partitionkey` carries ordering
scope as `<declared-prefix>/<identifier>`, preventing unbounded subject and ACL
cardinality. Ordering outside one partition key is explicitly undefined.

The initial portable envelope maximum is 256 KiB, including metadata and data.
Larger evidence belongs in object storage; the message carries an immutable
reference, digest, size, media type, and entitlement metadata. Broker profiles
may support larger messages but cannot increase this application contract.

## Retention contract

| Class | Delivery | Duration | Meaning |
| --- | --- | --- | --- |
| `Command` | Work queue | 7 days | Retain until acknowledged or the maximum age expires |
| `WorkflowEvent` | Retained stream | 30 days | Minimum replay window for ordinary workflow facts |
| `AuditEvent` | Retained stream | 365 days | Minimum replay window for material audit facts |

A command cannot select an event retention class, and an event cannot select
command retention. A managed broker profile must express equivalent semantics
or fail conformance explicitly; silent truncation, early expiry, or conversion
of retained events to destructive work queues is not portable behavior.

## Producer flow

1. Domain/application code selects a versioned payload type and immutable
   schema URI.
2. The producer supplies stable message, correlation, causation, idempotency,
   partition, occurrence-time, and trace values.
3. `MessageEnvelope::from_payload` validates metadata and serializes the typed
   payload as JSON.
4. `MessageEnvelope::to_json` produces the structured bytes for an outbox or
   transport adapter.
5. The message definition verifies schema, partition namespace, producer
   authority for events, and encoded size.
6. A future outbox implementation persists those exact bytes atomically with
   the local state transition.

The compact JSON encoder normalizes object-key order so the same validated
envelope has stable bytes. JSON object order remains semantically irrelevant;
consumers compare message identity and canonical content rather than raw
transport framing.

The contract never generates an ID or timestamp. A retry therefore cannot
silently become a new command, and deterministic tests do not depend on a
clock, random source, locale, or network.

## Consumer flow and failure semantics

1. A transport adapter passes untrusted bytes to
   `MessageEnvelope::from_json`.
2. JSON and CloudEvents parsing must succeed.
3. EdgeAgent rejects CloudEvents 0.3, missing required attributes, unsupported
   message-type syntax, invalid identifiers, non-string extensions, malformed
   trace context, missing schema identity, and non-JSON data.
4. The registry rejects unknown major versions and mismatched schema,
   partition, event producer, or size policy.
5. The consumer requests its expected payload type through
   `MessageEnvelope::payload`; type mismatch is a payload-decoding failure.
6. Only a validated envelope proceeds to schema compatibility,
   authorization, inbox deduplication, and domain handling.

Envelope and payload validation failures are permanent failures. A future
consumer will quarantine them with bounded, redacted diagnostics rather than
retry them blindly. Transport outage and acknowledgement loss are separate
transient failures and do not change message identity.

Unknown CloudEvents extension attributes survive SDK decoding and encoding.
Consumers ignore extensions they do not understand unless a payload or routing
contract explicitly requires them. Unknown major message types still fail at
the consumer capability boundary; this envelope validates naming, not consumer
support.

## Compatibility and evolution

Message types carry the payload major version. Meaning does not change within
a major version. Compatible evolution adds optional payload fields before any
producer requires consumers to understand them. Breaking changes publish a new
type and schema URI, deploy tolerant consumers first, then producers, and keep
the older contract until its retention and replay window closes.

The CloudEvents version, extension names, identifier constraints, and golden
fixture are compatibility surfaces. Changing one requires explicit migration
analysis and, when semantics change, a superseding architecture decision.

## Current limitations

This increment deliberately does not register domain messages before their
payloads exist. Each future payload change adds its `MessageDefinition` beside
the typed contract and fixture, then composes the definitions needed by each
process. It also does not define generated JSON Schemas, a schema registry,
NATS bindings, outbox/inbox tables, retry scheduling, acknowledgement,
quarantine operations, replay tooling, or telemetry export. Those are
independently reviewable M02 increments built on this contract.

The contract verifies that `dataschema` is an absolute URI but does not yet
validate `data` against the referenced schema. Typed Serde decoding and golden
fixtures provide the current payload check; generated schema validation must
arrive with the first public domain payload.
