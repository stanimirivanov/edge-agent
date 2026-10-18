# ADR-0029: Export bounded correlated event-spine observations

- Status: Accepted
- Date: 2026-10-18
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Conformance roots export the existing classified operation events as OTLP
  traces and the two bounded metric instruments. Application/port/facade crates
  acquire no exporter SDK dependency.
- Link validated envelope W3C context rather than rewriting accepted bytes.
  Causal facts preserve the winning command's carrier and causal identifiers.
- Export is best-effort, bounded and independent of commit/settlement results.
  Inspect decoded requests from actual independent workers; collector readiness
  is not evidence of exported observations.

## Context

The relay and handler already classify publication, persistence, handling and
confirmed acknowledgement through `edgeagent-telemetry`. Without a subscriber
or recorder, those facades intentionally do nothing. M02 needs exported evidence
across real processes, without adding a production monitoring platform or making
business correctness depend on telemetry availability.

## Decision

Install an opt-in allowlisted tracing subscriber in the finite conformance root.
It projects only `edgeagent.event_spine.operation` events on the canonical target
into an OpenTelemetry completed-stage span, counter increment and duration sample.
This is not a general log/metric bridge; it does not install a second facade
metrics recorder or count arbitrary tracing events. Existing applications remain
exporter-neutral. Producer acceptance emits a classified persistence observation;
confirmed fixture rollback emits `failed`, never a fabricated committed success.

Stage/outcome dimensions remain the four-by-five closed taxonomy. Resource
fields contain only component and worker role. PID is trace-only. Approved
message/correlation/causation/transport fields remain span attributes; payloads,
subjects, keys, failure/source text and baggage cannot enter the projection.
Unknown debug fields are not formatted. Optional validated `tracestate` joins
the facade's context; it is used in the span link, not as a metric dimension or
span attribute. Vendor state still requires environment-appropriate data policy.

Extract only `traceparent`/`tracestate` through the SDK W3C propagator. Each
completed observation starts an independent span linked to that carrier. Causal
facts copy the original carrier instead of the fixture default. This preserves
exact retry bytes and does not pretend a stored message was parented by a newly
created retry span. Message causation attributes retain the direct domain edge.
Malformed/untrusted envelopes get no guessed link. Stage intervals reconstruct
measured elapsed durations near export; they are not synchronized distributed
critical-path measurements or a parent/child trace tree.

Use OTLP/HTTP protobuf with explicitly pinned OpenTelemetry 0.33.1 API/SDK,
exporter and test wire models; Reqwest 0.13.5 supplies the blocking HTTP client.
Tracing-subscriber 0.3.23 supplies the layer; Axum 0.8.9 and Prost 0.14.4 decode
owned test requests without hand-writing HTTP or protobuf. Default features are
disabled and only used capabilities are enabled. These maintained upstream
projects use Apache-2.0 and/or MIT licenses already allowed by repository policy.
The conformance projection, stable facade, OTLP protocol and protected proofs are
the replacement boundary, not application-owned SDK interfaces. See the
[official exporter API](https://docs.rs/opentelemetry-otlp/0.33.1/opentelemetry_otlp/)
and [SDK lifecycle](https://docs.rs/opentelemetry_sdk/0.33.1/opentelemetry_sdk/).

## Alternatives considered

- Add SDKs to the telemetry facade or application crates: breaks exporter-neutral
  ownership and expands independently resolved experiment graphs unnecessarily.
- General-purpose facade bridge/general log exporter: adds unused surface and
  risks unbounded or private dimensions. The existing classified event already
  carries exactly the two measurements required here.
- Rewrite trace parents at every publication/retry: changes immutable envelope
  bytes and can fabricate lineage after uncertain outcomes.
- Assert collector health or install a dashboard: proves neither exported values
  nor failure independence. The tests inspect real decoded OTLP requests instead.
- Block processing on exporter success: makes diagnostic availability change
  business/storage/settlement behavior and encourages unsafe replay.

## Consequences

Tooling alone gains one direct internal edge to the exporter-neutral facade and
the reviewed SDK/HTTP/test dependencies. Root resolution grows; independent M08
resolutions remain separate. Existing duplicate-version warnings require review,
not blanket skips or forced incompatible unification. No dependency-policy
exception, new registry, product image/deployable or published event schema is added.

SDK background export uses a 64-span queue, batches of eight, one-second schedule,
16 span attributes, one link and no span events. Each metric instrument has at
most 20 dimension combinations; duration uses six explicit boundaries plus its
overflow bucket. Oversized/unknown fields or classifications are dropped, not
promoted into arbitrary labels. Export request bodies are limited to 256 KiB.

## Compatibility and migration

The facade adds optional vendor-state diagnostics without changing metric names,
dimensions, public constructor signatures or wire schemas. Facts now propagate
their actual command context; previously accepted bytes remain untouched. SQL,
adapter migrations and offline query descriptions are unchanged. Use disposable
proof schemas as before. Without the opt-in exporter endpoint, workers preserve
their no-export behavior. Roll back using old tooling with fresh proof fixtures;
no production database migration or retained-history rewrite is needed.

## Security and operations

**[Prerequisite from M10]** Accept only an explicit IPv4 loopback HTTP endpoint
with a nonzero port and no path, credentials, query or fragment. Disable proxies,
redirects and both HTTP/OTLP retries; reject ambient `OTEL_*` configuration when
export is enabled. Build blocking transport before Tokio starts. Use an empty
resource builder, not host/environment detectors. No exporter credentials or
public/production endpoint are introduced.

HTTP connect/request deadlines are 250 ms. Flush runs after runtime shutdown
without replacing the original operation result. Trace shutdown allows one
second. SDK 0.33's meter provider ignores its timeout argument; its periodic
reader uses a five-second control deadline. We therefore also bound actual HTTP
I/O and install no observable callbacks. Exporter degradation prints only a
category-safe diagnostic. Queue overflow/failed export may lose observations;
telemetry is not an audit log, delivery guarantee or replay authority.

The owned loopback receiver bounds bodies to 256 KiB and retains at most 64
decoded requests/raw bodies. It supports acceptance, HTTP 503 rejection and a
controlled withheld response. Cleanup releases handlers and stops/aborts owned
tasks. No cloud collector, credentials, model, market data or live trading runs.
Production TLS/IAM, deployment, backend retention/sampling, dashboards, backlog
gauges and SLO/load qualification remain their owning milestones.

## Validation

Three pure tests protect endpoint/resource bounds, all twenty classifications,
optional W3C state and rejection of invalid/unknown/debug fields. Two protected
process proofs inspect real OTLP messages from actual child PIDs, match metrics
to classified spans, verify non-default W3C links and causal fact context, reject
private sentinels, and cover rollback, retry, duplicate and poison quarantine.
Rejected/stalled export must preserve committed rows, published intent and
confirmed settlement; a subsequent healthy process exports again. The existing
process target grows to twelve cases and the independent protected floor to 55.
The canonical process gate admits two scenarios at a time to bound owned broker,
database and worker pressure; each case keeps its internal contenders and deadlines.
Discovery remains distinct from executed integration evidence. Full crash/wire
ACK/broker restart qualification remains M02 slice 7.
