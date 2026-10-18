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
- M02's finite conformance roots export real OTLP/HTTP observations through an
  opt-in, bounded subscriber. The protected process suite inspects decoded wire
  data and proves failure independence; this is not production monitoring.

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
empty rather than guessed. Optional validated `tracestate` travels with the W3C
carrier; the proof exporter uses it only in span links, never metric dimensions.

The outbox relay constructs identity-bearing context only after the stored
envelope, registry policy, and denormalized columns have all passed validation.
Rejected records use `EventSpineContext::without_identity`: only bounded stage
and outcome tokens, numeric attempt, and elapsed duration remain. No raw stored
source, ID, type, payload, or failure text enters a rejection event, even when
the quarantine write fails. `from_stored_identity` is retained for compatibility,
but does not validate its arguments; callers must independently establish trust.

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

## Conformance exporter configuration and lifecycle

[ADR-0029](../decisions/0029-export-bounded-correlated-event-spine-observations.md)
records the boundary and dependency review. Only the tooling worker installs the
OpenTelemetry SDK. Its allowlisted subscriber projects the existing classified
operation event into one completed-stage span, counter increment and duration
sample. It is not a general tracing-log exporter or metrics-facade bridge; no
second recorder double-counts those events. The facade and application crates
remain independent of exporters.

By default no export occurs. A proof parent may set
`EDGEAGENT_PROOF_OTLP_ENDPOINT=http://127.0.0.1:<owned-port>` in its child-only
environment. Only that explicit IPv4 loopback origin is accepted: no DNS,
credentials, paths, query, fragment or remote host. When enabled, ambient
`OTEL_*` settings are rejected. Worker environment clearing, disabled proxies,
redirects and retries prevent unrelated configuration from redirecting evidence.
The automated receiver binds an ephemeral listener without a close/rebind race.
Never point this tooling at production or reuse public fixture passwords there.

Resources contain only component and bounded worker role. PID is trace-only.
Metric points carry exactly `stage`/`outcome`; each instrument's cardinality is
limited to 20. The duration histogram uses boundaries 0.001, 0.01, 0.1, 1, 5 and
30 seconds plus the overflow bucket. These are measurement buckets, not SLOs.
The trace queue holds 64 spans, exports batches of eight on a one-second schedule,
and caps spans at 16 attributes, one link and no span events. Request bodies are
limited to 256 KiB. Field allowlists and size/type checks drop unknown or invalid
observations; arbitrary debug/error values are never formatted.

The blocking HTTP client is created before Tokio, with 250 ms connection/request
timeouts and no HTTP or OTLP retries. Export runs on SDK-owned background threads,
not inside a domain/storage call. After intake finishes and the two-thread Tokio
runtime shuts down, the root explicitly shuts down providers. Trace shutdown
allows one second; the SDK 0.33 meter provider ignores its timeout argument and
its reader uses a five-second control deadline. Actual HTTP timeouts and the
absence of observable callbacks also bound that path. SDK reader/trace threads
and the blocking client's runtime are additional bounded process resources.
The existing parent deadline still applies. Flush errors emit only a category-safe
diagnostic and cannot replace the original worker result or authorize replay.

## W3C and observation meaning

The subscriber extracts only validated `traceparent`/`tracestate`, never baggage.
It creates independent stage spans linked to the immutable message's upstream
carrier. Causal facts copy the winning command's carrier and correlation, while
causation names that command. A retry does not rewrite durable bytes to inject a
new span parent. A malformed envelope gets no guessed W3C link. Vendor state is
opaque diagnostic context; deployments must review its privacy/retention policy.

Intervals reconstruct measured monotonic elapsed durations near observation
completion. They do not establish synchronized end-to-end latency or a
parent/child trace tree. OTLP metrics use delta temporality; this finite proof
does not establish aggregation/restart semantics for a production backend.
Telemetry may be dropped after queue saturation or failed export. It is neither
an immutable audit store nor proof of business completion.

## Verification and current limitations

Credential-free unit tests fix metric names, bounded dimension tokens, and the
approved diagnostic context. The existing PostgreSQL and event-spine conformance
tests compile and exercise the instrumented durable paths when their isolated
dependencies are available.

Run `cargo xtask verify --check event-spine-processes` with the isolated local
PostgreSQL profile and Docker or the pinned native NATS executable, as described
in [process acceptance](process-acceptance.md). Missing prerequisites fail.
The parent owns the loopback OTLP receiver; no collector installation, manual
environment configuration inside children or external backend is required.

Two required cases inspect decoded protobuf requests from real worker PIDs,
compare counters/histogram counts with stage spans, check non-default W3C links
and causal fact context, and reject private sentinels in raw wire bytes. They
cover success, explicit rollback, retry, duplicates and poison quarantine. A
second case rejects exports with HTTP 503 or withholds responses until the
client deadline; commits, duplicate suppression and confirmed ACKs must remain
unchanged. Healthy re-entry exports again, not by replaying prior observations.
The receiver retains at most 64 bodies/decoded requests, 256 KiB each before
decoding; decoded models add overhead. Cleanup releases held handlers and bounds
owned task shutdown. Default verification compiles/discovers T3 proofs, not their
service-backed outcomes.

No OpenTelemetry SDK or exporter is installed in product service binaries.
The proof does not instrument replay-control-plane details or expose
queue-depth gauges, consumer lag, outbox backlog, or quarantine depth because
those require a long-running service lifecycle and bounded polling policy.
Backend persistence, cloud/TLS/IAM deployment, retention/sampling, exporter load
and SLO qualification remain outside M02 slice 6. Full crash/wire-ACK-loss
qualification remains slice 7. No models or live execution are involved.
