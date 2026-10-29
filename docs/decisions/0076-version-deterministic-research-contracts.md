# ADR-0076: Version deterministic research contracts

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Define additive v1 request, artifact, abstention and lifecycle payloads.
- Validate canonical bytes before trusting untrusted DTOs.
- Keep policy, authentication, delivery and lifecycle execution outside these contracts.

## Context

M04 requires stable payloads shared by gateway, research and projector. Existing
research envelope fixtures demonstrate messaging but do not define product payloads.

## Decision

Use a tagged v1 record union, bounded canonical JSON and SHA-256 content identity.
Request scope and request ID identify admitted intent; event delivery IDs remain
separate. Freeze configuration and implementation digests, decision time and an
explicit sorted universe. Artifact ranking uses dimensionless integer ppm scores,
with score descending and instrument ascending ties. Trading-session lookback is
bounded to 1–252; this is a structural ceiling, not an approved strategy default.

Artifacts retain evidence digest/availability references and an explicit expiry.
Abstention reasons are typed terminal evidence-policy outcomes, not parse errors.
Lifecycle records append owner revisions and reference the immutable artifact;
corrections name a distinct replacement. Applications must authenticate owners,
check rights and prior state, verify referenced bytes, and enforce consecutive
revisions. Hashes alone establish none of these authorities.

## Alternatives considered

Reusing the messaging demonstration fixture would conflate envelope and product
compatibility. Adding a numeric library is unnecessary for integer wire values.
Generic JSON extension maps would conceal unreviewed fields and revisions.

## Consequences

Raw DTO serde parsing does not establish validity. Consumers use bounded decode
or validate canonical bytes before persistence/publication. JSON keys are sorted;
array order is contractual. Unknown fields, alternate versions and noncanonical
encodings fail closed. The local schema describes structure; cross-field rules
remain Rust validation obligations.

## Compatibility and migration

Existing envelopes and fixtures remain unchanged. No routes or consumers activate.
V1 explicitly represents scenario/risk illustrations as `not_qualified`; it does
not claim the complete future product artifact or fabricate entry/target values.
Qualified scenario fields require a separately reviewed contract version. Adding
reason or lifecycle variants requires compatibility review for exhaustive readers.

## Security and operations

No runtime, broker, database, provider or credential operations are introduced.
Payload diagnostics are constant and DTOs do not derive Debug. Reference tokens
are not URLs. The caller must enforce ingress size limits before DTO parsing.

## Validation

The default gate includes `research-contracts`: frozen payload round trips,
canonical identity, structural schema freshness and invalid input/redaction tests.
Run the same tests with serde_json map-order feature unification enabled.
