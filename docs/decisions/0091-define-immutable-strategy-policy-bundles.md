# ADR-0091: Define immutable strategy policy bundles

- Status: Accepted
- Date: 2026-10-30
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Define closed versioned strategy and independent policy declarations.
- Identify exact bounded canonical bytes with SHA-256.
- Keep approval and runtime qualification outside configuration identity.

## Context

M05 needs immutable configuration inputs shared by governance and policy enforcement.
M04 request configuration digests already bind opaque owner-selected configuration.

## Decision

Add research.configuration.v1 as a standalone contracts-only bundle. Declare
strategy requirements, compatibility, formula references, horizons, expiry, benchmark,
regime and invalidation alongside independent policy limits. Use existing canonical
JSON and Serde; reject unknown fields, duplicate keys and alternate encodings.
No defaults or mutable approval state enter the digest. All market-condition policy
inputs are mandatory; missing runtime evidence will require fail-closed enforcement.

## Alternatives considered

Mutable registry aliases would make replay ambiguous. Embedding policy in strategy
output would weaken independent control. New serialization libraries are unnecessary.

## Consequences

Governance and policy loaders can consume one immutable identity. Formula references
remain declarations: later loaders must resolve and qualify exact implementations.
No runtime selection or production threshold is established by the fixture.

## Compatibility and migration

Existing M04 records and digest meanings remain unchanged. No storage migration or
runtime activation occurs. Future loaders must explicitly opt into this contract.

## Security and operations

A content digest is neither approval nor evidence authority. Parsing is bounded and
errors are payload-safe. No provider, credential, network or deployment is added.

## Validation

Frozen bytes/digest, nested unknown fields, incompatible declarations, numeric and
selection bounds, duplicate keys, missing limits and redaction tests accompany the
contract. Run repository/architecture sensors and the full default gate.
