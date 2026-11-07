# ADR-0108: Enable bounded loopback OTLP in Gateway and the audit projector

- Status: Accepted
- Date: 2026-11-07
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use the Research export lifecycle for Gateway serving and projector broker intake.
- Observe committed admission, projection handling and broker settlement.
- Attribute observations by component and role while omitting caller and message identities.

## Context

[ADR-0107](0107-enable-bounded-otlp-in-research.md) proves the shared exporter in
Research. Operators also need to see whether the request reached durable admission
and whether the projector handled and settled the resulting facts.

## Decision

Enable the same optional exporter at each process root. Observe Gateway's
persistence after commit and distinguish fresh admission from an identical retry.
Observe the projector's classified handling and broker settlement separately.
Use the identity-free signal context for both to avoid exposing caller data.

## Alternatives considered

A general HTTP or database instrumentation layer would collect more fields and
would miss the distinction between accepted input and a confirmed durable result.

## Consequences

Export uses the existing stage and outcome vocabulary and per-process limits.
Collector unavailability can lose telemetry. Gateway and projector retain their
original durable behavior and query formats.

## Compatibility and migration

Both settings default to absent. Enable Gateway serving with
`EDGEAGENT_GATEWAY_OTLP_ENDPOINT` or projector broker intake with
`EDGEAGENT_PROJECTOR_OTLP_ENDPOINT`. Each accepts an explicit IPv4 loopback HTTP
origin. Remove the setting to disable export.

## Security and operations

The shared client uses the existing pinned ring provider in its own TLS config
to tolerate Cargo feature unification without installing a process-wide provider.
Export endpoints remain restricted to HTTP loopback. The new observations carry
only fixed stage and outcome fields, delivery attempt, elapsed time, and process
attribution.

## Validation

The real process journey inspects decoded OTLP on both accepting and rejecting
receivers. It checks bounded dimensions, redacted fields, the retained request,
settled broker deliveries and byte equality of the projected query outcome.
