# ADR-0010: Keep delivery metadata transport-neutral

- Status: Accepted
- Date: 2026-10-01
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- `DeliveryMetadata` carries only a stable opaque transport message key, a
  delivery route, and a positive attempt count.
- Broker-specific stream offsets, consumer sequences, and pending counts remain
  inside adapters; they are not prerequisites for another broker to implement
  the application-owned consumer port.
- The NATS adapter still rejects invalid stream identity or consumer sequence
  before returning a delivery. The change does not weaken protocol validation,
  settlement, inbox identity, or quarantine evidence.

## Context

The consumer port is intended to work across the local JetStream profile and
qualified managed-broker profiles. Its metadata constructor required a stream
sequence, consumer sequence, and pending count, although application handlers
use only the opaque message key, route, and delivery attempt. Some candidate
brokers cannot supply JetStream-equivalent counters. Requiring them in the port
would force adapters to invent values with false diagnostic meaning.

The NATS adapter derives a stable transport message key from stream name and
stream sequence. Those inputs still need validation before a delivery enters
the application. Removing them from the portable value must not turn malformed
broker metadata into a valid delivery.

## Decision

`DeliveryMetadata` contains three validated fields: `DeliveryMessageKey`,
`DeliverySubject`, and `DeliveryAttempt`. The key remains opaque to application
policy and stable across redelivery. The subject identifies the route needed
for quarantine evidence and authorized replay. The attempt remains positive
and supports bounded retry policy; adapters must document how they obtain it.

Each broker adapter validates its own protocol fields before constructing this
value. JetStream requires a nonempty stream name, positive stream and consumer
sequences, a representable positive delivery count, and a valid delivery
subject. It derives the key from stream name and stream sequence, then discards
the broker-only counters at the port boundary. Pending count does not influence
handling policy and is not part of this contract.

Adapter-specific diagnostics may later expose broker counters through bounded
telemetry, but never as required application inputs or metric labels with
unbounded cardinality. No generic metadata bag is introduced.

## Alternatives considered

### Keep JetStream counters in the portable type

- Benefits: Existing getters and constructor remain unchanged.
- Costs and risks: Other adapters must fabricate unsupported values, and
  application policy can accidentally depend on one broker's offsets.
- Reason not selected: Diagnostic convenience is not a portable requirement.

### Make every broker counter optional

- Benefits: Existing diagnostic fields remain accessible for JetStream.
- Costs and risks: The public port still teaches broker-specific vocabulary and
  requires every handler to interpret absence correctly.
- Reason not selected: No current handler needs these fields.

### Use an opaque broker metadata map

- Benefits: Adapters can expose arbitrary diagnostics without port changes.
- Costs and risks: It creates an untyped, potentially sensitive and
  high-cardinality escape hatch across the application boundary.
- Reason not selected: Adapter-owned telemetry is a narrower future extension.

## Consequences

### Positive

- Managed-broker adapters can implement the same delivery contract without
  pretending to have JetStream stream and consumer sequences.
- Handler tests construct only semantically required metadata.
- NATS protocol faults still halt intake before handler-owned settlement.

### Negative

- The public Rust constructor and exports change; external callers using the
  removed counters must update at compile time.
- Broker-specific diagnostics are not available from `DeliveryMetadata`.

### Neutral or follow-up

- The opaque message key is a transport-delivery identity for quarantine and
  replay, not the CloudEvents `(source, id)` business-effect identity.
- Each new broker adapter must prove stable redelivery identity and positive
  attempt semantics in its conformance suite.

## Compatibility and migration

This is a compile-time Rust API change within the unreleased `0.1.0` workspace.
Update constructors, fakes, and any external callers together. CloudEvents
bytes, PostgreSQL records, broker subjects, quarantine identity, and persisted
meaning do not change, so no wire or database migration is needed. Rollback
requires rebuilding callers against the earlier library API, not changing data.

## Security and operations

Malformed JetStream sequence metadata still produces `Protocol`, leaves the
raw message unsettled, and halts that consumer instance. Operators investigate
the broker configuration before replacement; they do not acknowledge or
terminally settle a message without trustworthy identity. The portable value
remains bounded and contains no payload. Adapter-only diagnostics must not log
credentials, raw message bytes, or unbounded metric labels.

## Validation

- Portable metadata tests prove distinct key, route, and attempt mapping plus
  text and positive-attempt validation without broker counters.
- NATS tests prove zero stream or consumer sequence is rejected and the
  protocol gate prevents another pull after invalid metadata.
- Existing inbox-handler tests prove duplicate, retry, quarantine, and
  acknowledgement-loss behavior with the narrowed port.
- Repository, architecture, Clippy, and workspace tests run through `make verify`.
