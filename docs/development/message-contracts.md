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
- Generated payload schemas, service-specific domain handlers, and operator
  replay remain separate M02 capabilities. PostgreSQL outbox, inbox, and inbound quarantine
  persistence implement transaction boundaries, while messaging adapters
  provide durable publication, delivery, and confirmed settlement.

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
features disabled. Protocol adapters consume the validated structured bytes
rather than redefine the envelope.

The crate keeps envelope construction and encoding in `messaging.rs`, shared
metadata and event checks in its private `validation` module, routing policy in
`routing.rs`, and exact-version lookup in its private `registry` module. Each
area owns a private error module, while large unit suites live in separate test
files. The crate-root re-exports remain the public Rust API; the internal file
layout does not alter wire compatibility.

`MessageDefinition` binds one exact major-version type to its schema, semantic
kind, owner, partition namespace, and retention class. `MessageRegistry`
rejects invalid or duplicate definitions and resolves untrusted envelopes only
by their exact type. This makes broker configuration and service authorization
derivable from code-reviewed contracts rather than duplicated strings.
Definition fields are private: callers use `command` or `event` constructors and
read-only accessors. `event` accepts `EventRetention::Workflow` or
`EventRetention::Audit`, so command retention cannot be assigned to an event.
Textual type, schema, and partition values remain validated at registry
construction. A registry then checks each envelope against those immutable
definitions without reparsing static policy. Standalone definition checks stay
defensive and validate the definition first.

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
| `tracestate` | Optional W3C vendor-state list, at most 512 bytes and 32 members |
| `data` | JSON payload; its meaning is owned by the declared schema |

Identifiers are limited to 128 bytes and use ASCII letters, digits, hyphen,
underscore, dot, or colon. The `type`, `dataschema`, and other bounded metadata
except `tracestate` are limited to 512 bytes of visible ASCII without spaces so
they remain safe in logs, broker headers, metrics, and cross-cloud adapters.
`tracestate` instead follows the W3C list grammar: up to 32 comma-separated
members, each a unique lowercase vendor key and a bounded printable-ASCII value.
Spaces and horizontal tabs around members, including empty members and an empty
header value, are accepted; controls and malformed nonempty members are rejected.
The complete `tracestate` value is limited to 512 bytes. The metadata limits
apply when both producing and decoding envelopes and to static routing definitions
where relevant. Payload content is not copied into validation errors.

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
Transport adapters reject raw structured input above 256 KiB before handing it
to an application consumer. Direct decoders enforce the same limit before JSON
or CloudEvents parsing, including whitespace-padded input. Routing still
checks the encoded size of producer-built envelopes. An exactly 256 KiB valid
JSON input is accepted by the raw boundary; an additional byte is not.
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
6. The PostgreSQL outbox persists those exact bytes atomically with the local
   state transition. A relay later revalidates them and uses `MessagePublisher`
   to wait for durable transport acknowledgement.

The compact JSON encoder sorts object keys recursively, independent of
`serde_json/preserve_order` feature unification. The same validated envelope
therefore has stable bytes across default and feature-unified builds. JSON
object order remains semantically irrelevant; consumers compare message
identity and canonical content rather than raw transport framing.

The contract never generates an ID or timestamp. A retry therefore cannot
silently become a new command, and deterministic tests do not depend on a
clock, random source, locale, or network.

## Consumer flow and failure semantics

1. A transport adapter rejects raw bytes over 256 KiB before constructing a
   portable delivery. Accepted bytes remain untrusted.
2. The handler passes accepted bytes to `MessageEnvelope::from_json`; JSON and
   CloudEvents parsing must succeed before domain work.
3. EdgeAgent rejects CloudEvents 0.3, missing required attributes, unsupported
   message-type syntax, invalid identifiers, non-string extensions, malformed
   trace context, missing schema identity, and non-JSON data.
4. The registry rejects unknown major versions and mismatched schema,
   partition, event producer, or size policy.
5. The consumer requests its expected payload type through
   `MessageEnvelope::payload`; it deserializes from the borrowed JSON value
   without cloning the complete payload tree. Type mismatch remains a
   payload-decoding failure.
6. Only a validated envelope proceeds to schema compatibility and authorization.
7. The PostgreSQL inbox records `(consumer_name, source, id)` before domain
   handling. The consumer applies a first delivery and any resulting outbox
   message in the same transaction; an identical duplicate skips domain work.
8. The one-shot delivery is acknowledged only after commit. A rollback or
   dropped delivery permits redelivery, while acknowledgement loss after commit
   resolves to an inbox duplicate. Transient failures request bounded delayed
   redelivery; terminal settlement follows durable quarantine persistence.

Direct decoding of oversized raw input returns
`MessageContractError::EnvelopeTooLarge` with only byte counts. An oversized
transport delivery instead fails as `ConsumeErrorKind::Protocol` before a
handler can receive it; the adapter leaves the raw message unsettled and stops
intake for operator investigation. Malformed input within the limit returns
`EnvelopeDecoding` with parser line and column, not the parser's free-form text.
Invalid `type` or `dataschema` metadata returns `InvalidMetadata`. These are
permanent validation failures. Consumers can retain exact bytes of accepted
deliveries with bounded reason codes before requesting terminal settlement
instead of retrying them blindly. Oversized raw deliveries have no committed
quarantine evidence. Transport outage and acknowledgement loss are separate
transient failures and do not change message identity.

Contract and routing errors expose stable categories, safe field names, counts,
and parser coordinates through `Display` and `Debug`. They do not retain
serializer, deserializer, SDK, unsupported-type, or mismatched metadata text.
Authorized operators inspect exact quarantined bytes through the evidence path,
not through an error chain. The Rust error-variant change is documented in
[ADR-0012](../decisions/0012-keep-message-contract-diagnostics-payload-safe.md).

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
This refactor changes only the Rust API for routing definitions: replace direct
field reads with accessors and pass `EventRetention` to `MessageDefinition::event`.
All workspace callers have been migrated; no CloudEvents bytes, subjects,
retention durations, or stored definition records change. The constructors
remain `const`, while startup validation still rejects invalid textual values.
`tracestate` validation now follows the previously declared W3C contract:
conforming whitespace and empty members are accepted, while previously accepted
malformed entries or duplicate keys fail as permanent invalid metadata. Existing
messages with valid W3C state need no wire migration.
The inbound byte and metadata limits tighten previously accepted inputs without
changing field encoding; see [ADR-0011](../decisions/0011-bound-untrusted-envelopes-before-decoding.md).

## Current limitations

The current contract deliberately does not register domain messages before their
payloads exist. Each future payload change adds its `MessageDefinition` beside
the typed contract and fixture, then composes the definitions needed by each
process. The NATS publisher binding and PostgreSQL outbox are documented in the
[messaging adapter guide](messaging-adapters.md) and
[outbox guide](postgres-outbox.md). The transactional consumer boundary is
documented in the [inbox guide](postgres-inbox.md). Generated JSON Schemas, a
schema registry, stream provisioning, long-running relay and consumer service
loops, quarantine replay tooling, and telemetry export remain independently
reviewable M02 increments built on this contract. Outbound relay mechanics are documented in the
[relay guide](outbox-relay.md).

The contract verifies that `dataschema` is an absolute URI but does not yet
validate `data` against the referenced schema. Typed Serde decoding and golden
fixtures provide the current payload check; generated schema validation must
arrive with the first public domain payload.
