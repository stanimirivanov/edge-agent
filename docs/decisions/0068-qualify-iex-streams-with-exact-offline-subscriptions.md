# ADR-0068: Qualify IEX streams with exact offline subscriptions

- Status: Accepted
- Date: 2026-10-27
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Select Alpaca IEX trades/minute bars for offline protocol conformance only.
- Preserve venue-only coverage, exact pinned subscriptions and source precision.
- Keep transport charging ahead of provider decoding; fail closed on corrections.
- Activate no vendor, daemon mode, credentials or evidence readiness.

## Context

PR18 bounds WSS and charges frame arrivals without provider interpretation. The
maintainer approved IEX trades and minute bars for PR19. Historical Alpaca SIP
semantics cannot describe venue-only streaming. Existing sessions admit exactly
one evidence contract and do not establish source continuity or final evidence.

## Decision

Add a cohesive provider protocol adapter to the existing transport crate, using
its current JSON/raw-decimal and verified WSS dependencies. Add explicit Xiex
venue coverage, without changing the meaning of existing venue/discriminant values.
Pin the complete selection witness, exact symbol subscriptions and one contract
per session. Retain original mapping revision, provider fields, event precision,
receipt and local frame/item coordinates in transient typed adapter records.

Validate welcome, authentication and full subscription acknowledgements within
one finite deadline. Process automatic corrections/cancels even for unselected
symbols. Unsupported correction/cancel/updated-bar messages stop intake with an
explicit reconciliation category. Return no partial batch on any later failure.
No contiguous source sequencing, deduplication, correction history or eligibility
is inferred from arrival order. PR20 must establish those consumer requirements
or abstain; these adapter results are not canonical accepted observations.

## Alternatives considered

- Reuse consolidated SIP contracts: rejected because coverage differs.
- Import a full trading SDK: unnecessary weight and hidden transport/accounting.
- Ignore corrections or overwrite immutable evidence: violates evidence integrity.
- Add combined trade/bar admissions now: would redesign accounting beyond PR19.
- Activate a provider/test endpoint: outside approved offline-only scope.

## Consequences

The library proves wire interpretation, exact subscriptions and bounded failure
behavior, not rights, readiness or live execution. No new dependencies, migrations,
runtime modes, environment controls or broker path are added. Raw condition codes
remain provider data pending authenticated semantics; no guessed eligibility map.

## Compatibility and migration

Additive Rust API and Xiex venue value. Existing venue encodings and daily/wire
contracts stay unchanged. Old consumers can still reject unsupported Xiex scope;
PR20 must compose format-aware acceptance rather than relabeling its coverage.
Rollback stops construction; retain all charged accounting and unresolved ownership.

## Security and operations

No credential loading or live runtime mode is installed. Preserve transport failure
causes for privileged inspection, never automatic source-chain logging. Independent
owner authorization is still required for account quiescence and licensed cleanup.
Rollback leaves charged counters and unresolved reservations intact.

## Validation

Protected synthetic protocol/TLS tests cover state, subscription scope, exact
decimals/times, fragments/controls, provider/reconciliation failures, uncertainty,
cancellation, abandonment and diagnostic redaction. Run the default harness and
the new `alpaca-iex-conformance` check. No vendor credentials or calls are used.

- [Alpaca IEX guide](../development/alpaca-iex.md)
- [M03 plan](../development/plans/m03-acquisition.md)
- [ADR-0067](0067-bound-wss-frames-before-provider-decoding.md)
