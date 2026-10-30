# ADR-0093: Isolate experimental Research results

- Status: Accepted
- Date: 2026-10-30
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Reuse deterministic evaluation for a separate experimental result type.
- Label the entire output as non-approved and exclude publication capabilities.
- Compose the command before Research database or broker initialization.

## Context

M05 needs experiments that do not depend on approval and cannot accidentally publish.
The existing owner loader already authenticates and bounds retained evidence reads.

## Decision

Add a pure experimental evaluator returning only research.experiment.v1 envelope
bytes, with publication_eligible false and a nested canonical result for numeric
comparison. Keep RenderedOutcome private inside evaluation; expose no conversion.
The publication constructor requires its existing distinct type. Add offline-experiment
before store/broker composition, using existing owner-ready evidence and M04 bindings.
Non-ready, unauthorized or unavailable owner input produces no experiment result.

## Alternatives considered

A mode flag on the publication worker could accidentally persist experiments. Returning
ordinary artifact bytes would erase their non-approved status. A new strategy, loader,
database or runtime framework is unnecessary.

## Consequences

Experiments share qualified calculations without claiming approval or performance
validation. Extracting nested JSON does not authorize publication. Durable trials and
promotion remain later M05 work. No public ingress or simulation eligibility is added.

## Compatibility and migration

Existing Research records and commands are unchanged. No schema/dependency migration
is required. Remove the additive command and type to roll back; existing records remain.

## Security and operations

Owner authorization, rights and retained-byte integrity checks remain mandatory. The
experimental command receives no Research store, outbox or broker port. Input and
owner execution retain existing bounds; default process errors omit payload details.

## Validation

Test numerical parity, explicit non-approved status, request/expiry mismatch, owner
failure and malformed input. Actual command tests require no Research database or
broker. A compile-fail doctest rejects experiments at the publication constructor.
Run repository/architecture sensors and the full default gate.
