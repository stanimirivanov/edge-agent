# ADR-0107: Enable bounded loopback OTLP export in Research

- Status: Accepted
- Date: 2026-11-07
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Opt in to loopback OTLP from the Research broker worker and relay.
- Share the classified event projection and lifecycle with conformance workers.
- Keep collector availability outside durable handling and settlement.

## Context

The real Research broker worker and relay run the durable paths exercised by
M04 and M05. The conformance worker already qualifies a bounded exporter, but
that evidence did not cover the Research binary or its settlement path.

## Decision

Share the [ADR-0029](0029-export-bounded-correlated-event-spine-observations.md) exporter behind the telemetry crate's optional `otlp` feature.
The Research composition root can install it for the broker worker and relay.
Application and adapter callers continue to record through the signal facade.
Research records the handling disposition and then observes the actual broker
settlement result. A successful classification includes receipt recovery.

## Alternatives considered

Duplicating the conformance subscriber would require maintaining the bounds and
redaction allowlist twice. A generic tracing exporter would broaden the set of
fields that could cross the collector boundary.

## Consequences

Opted-in roots create bounded SDK background workers before entering the
application runtime and flush after it exits. The chosen transport, field
allowlist, dimensions, queue limits and control deadlines are those of ADR-0029.
Collector failures can drop diagnostics and add bounded shutdown latency.

## Compatibility and migration

Research export requires `EDGEAGENT_RESEARCH_OTLP_ENDPOINT`. Conformance
keeps its own setting and instrumentation scope. Remove the Research setting
to disable export. Stored messages, query results and receipt bytes retain their
existing formats.

## Security and operations

The endpoint accepts only an explicit IPv4 loopback HTTP port. Ambient OTEL
overrides, proxies, redirects and retries are disabled or rejected at the
exporter boundary. Operators control access to the local collector because
approved message correlation fields are present in traces.

## Validation

The Research process gate exercises disabled export, a collector that accepts
requests without responding, and healthy export after redelivery. It checks
canonical outcomes, confirmed settlement, bounded labels and redaction from
decoded OTLP requests. The existing conformance process gate exercises the
shared projection across its original worker roles.
