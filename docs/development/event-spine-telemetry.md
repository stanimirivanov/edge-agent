# EdgeAgent event-spine telemetry

## TL;DR

- `edgeagent-telemetry` defines one exporter-neutral signal contract for durable
  message publication and handling.
- Counters and duration histograms use only bounded `stage` and `outcome`
  dimensions.
- Structured events carry validated message, correlation, causation, transport,
  and W3C parent identifiers but never payloads or idempotency keys.
- Telemetry is emitted only after an operation has a classified result; it never
  changes retry, persistence, acknowledgement, or quarantine behavior.
- Deployable composition roots remain responsible for installing and flushing
  OpenTelemetry-compatible recorders and subscribers.

## Boundary and purpose

The event spine needs consistent signals across storage and transport adapters
without importing a cloud monitoring SDK into application or domain code.
`edgeagent-telemetry` centralizes stable metric names, dimensions, outcome
tokens, and diagnostic fields. The crate depends only on application contracts,
the portable messaging port, and exporter-neutral `metrics` and `tracing`
facades.

The outbox relay emits broker publication results and the following durable
outbox transition. The transactional inbox handler emits classified
inbox-processing and quarantine-persistence results, confirmed acknowledgement
policy, and its final handling outcome. Each observation includes the one-based
attempt and elapsed duration.

## Signal contract

| Signal | Type | Dimensions | Meaning |
| --- | --- | --- | --- |
| `edgeagent.event_spine.operations` | Counter | `stage`, `outcome` | Completed classified stage operations |
| `edgeagent.event_spine.operation.duration` | Histogram, seconds | `stage`, `outcome` | Elapsed time until the classified result |

The bounded stages are `publication`, `persistence`, `handling`, and
`acknowledgement`. Outcomes are `succeeded`, `duplicate`, `retry_scheduled`,
`quarantined`, and `failed`. A publisher duplicate is successful broker
deduplication, while an inbox duplicate is successful suppression of repeated
domain work; both use the `duplicate` outcome.

Structured events use the `edgeagent.event_spine` target and the stable event
name `edgeagent.event_spine.operation`. When available, they include message ID,
source, type, correlation ID, causation ID, validated `traceparent`, transport
message key, delivery attempt, stage, outcome, and duration. Missing fields are
empty rather than guessed.

## Security and cardinality

Message payloads, subjects, idempotency keys, failure text, user identifiers,
instrument identifiers, and order identifiers cannot be supplied to the
telemetry function. They therefore cannot accidentally become logs or metric
labels through this boundary. Metric labels are closed Rust enums and cannot be
derived from untrusted envelope content.

Message and trace identifiers are intentionally permitted only as diagnostic
event fields. Exporters must apply the environment's retention, access, and
sampling policy. Correlation identifiers provide diagnostics, never authority.

## Failure semantics

Telemetry uses the facades' no-op behavior when no recorder or subscriber is
installed. Recording does not return an error and cannot convert a failed domain,
storage, broker, or settlement operation into success. A `failed` persistence
signal means no durable completion was confirmed; normal retry or process
recovery policy still owns the message.

Publication is observed when the publisher returns. Persistence is observed
after the associated outbox, inbox, or quarantine operation succeeds or fails.
A handler failure that rolls back atomic inbox processing emits `failed`
persistence before retry or quarantine resolution; it does not imply a
committed inbox record.
Acknowledgement is observed only after the broker settlement future returns.
The final handling observation therefore describes the caller-visible
`HandlingOutcome` or `HandlerError`, not an intermediate intention.

## Verification and current limitations

Credential-free unit tests fix metric names, bounded dimension tokens, and the
approved diagnostic context. The existing PostgreSQL and event-spine conformance
tests compile and exercise the instrumented durable paths when their isolated
dependencies are available.

This increment does not install an OpenTelemetry SDK, exporter, sampler,
subscriber, or metrics recorder in service binaries. It also does not expose
queue-depth gauges, consumer lag, outbox backlog, or quarantine depth because
those require a long-running service lifecycle and bounded polling policy.
