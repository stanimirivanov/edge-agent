# ADR-0080: Render ranked research outcomes

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Bind requests to explicit selected policies before deterministic evaluation.
- Rank all evidence-qualified momentum scores and render canonical v1 outcomes.
- Retain whole-request abstention and explicit unavailable scenario/risk output.

## Context

M04 has versioned contracts, frozen-evidence eligibility and momentum. It needs a
callable composition returning reproducible outcome bytes before persistence and
publication are introduced. Numeric and reference policy must stay inward of DTOs.

## Decision

Keep ranking in research-domain and wire composition in research-application.
Use score descending and instrument ascending ties. Zero and negative scores
remain eligible, as established by ADR-0079. Retain ADR-0078 whole-request evidence
abstention: no partial universe or unreviewed threshold is selected. No additional
score exclusion means the reserved no-eligible-candidate reason is not emitted.

Require canonical hexadecimal instrument identity tokens in this application
profile. Match request cutoff, configuration/implementation, universe and lookback
to selected policy. Use existing v1 validation and canonical encoding. The renderer
has a fixed versioned ASCII template; qualified scenario outputs remain unavailable.
Evidence references retain exact digests and maximum selected availability.

## Alternatives considered

An implicit positive-return threshold would add strategy policy not yet approved.
Partial-universe evaluation would weaken required-evidence semantics. An LLM or
locale-sensitive template is unnecessary. Importing wire DTOs into domain ranking
would couple pure policy to transport. New numerical/serialization libraries are
unnecessary; the existing dependencies suffice.

## Consequences

The callable evaluator produces an immutable artifact or abstention, never a
published event. Malformed request, mismatched binding, invalid expiry and numeric
failure remain errors. A new crate has observable behavior immediately, with no
unused ports or speculative runtime framework.

## Compatibility and migration

No wire schema or existing fixture changes. The application profile is narrower
than the general contract token syntax. Explicit expiry is an immutable evaluation
input, must be part of admitted configuration and must not exceed entitlement.
Future configuration loading must authenticate and verify all selected policy bytes;
matching a supplied digest alone does not do that. Replay must retain expiry,
formula and renderer revisions. No stored-data migration is needed.

## Security and operations

Caller/owner/audience authentication, evidence integrity, reference resolution and
later-use reauthorization remain outer application obligations. No clock, model,
provider, credential, database, broker or deployment operation is added. Rollback
removes this additive composition before later consumers adopt it; no state changes.

## Validation

Protect six research-rendering tests in default verification, including exact
canonical fixtures under map-order feature unification, permutations, bindings,
whole-request abstention, expiry/range failure and content identity. Run both
architecture sensors, full default verification and applicable supply-chain checks.
