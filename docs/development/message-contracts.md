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
- NATS subjects, generated payload schemas, outbox/inbox persistence, retries,
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

## Producer flow

1. Domain/application code selects a versioned payload type and immutable
   schema URI.
2. The producer supplies stable message, correlation, causation, idempotency,
   partition, occurrence-time, and trace values.
3. `MessageEnvelope::from_payload` validates metadata and serializes the typed
   payload as JSON.
4. `MessageEnvelope::to_json` produces the structured bytes for an outbox or
   transport adapter.
5. A future outbox implementation persists those exact bytes atomically with
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
4. The consumer requests its expected payload type through
   `MessageEnvelope::payload`; type mismatch is a payload-decoding failure.
5. Only a validated envelope proceeds to schema compatibility,
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

This increment deliberately does not define broker subject names, command
ownership, retention, payload-size limits, generated JSON Schemas, a schema
registry, NATS bindings, outbox/inbox tables, retry policy, acknowledgement,
quarantine, replay, or telemetry export. Those are independently reviewable
M02 increments built on this envelope.

The contract verifies that `dataschema` is an absolute URI but does not yet
validate `data` against the referenced schema. Typed Serde decoding and golden
fixtures provide the current payload check; generated schema validation must
arrive with the first public domain payload.
