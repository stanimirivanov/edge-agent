# ADR-0031: Define the supported market evidence profile

- Status: Accepted
- Date: 2026-10-18
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Adopt `us-equity-daily-v1`: bounded raw daily evidence, explicit provider-day
scope, immutable point-in-time provenance and fail-closed rights. Select Massive
as the first technical adapter candidate, not as an approved live entitlement.
Use synthetic public fixtures; require separate verified rights and live proof.

## Context

M02 qualifies recoverable delivery, but market-data is still a bootstrap shell.
Domain types and provider adapters need one supported evidence meaning before
implementation. Historical retrieval, adjustment defaults and API access can
otherwise masquerade as point-in-time provenance or permission to store data.

## Decision

Use the canonical [market-data profile](../development/market-data.md) for M03.
It defines liquid USD common-equity/ETF listing coverage, stable identity,
reference authority, exact numeric bounds, calendar/session semantics, freshness,
resource limits and retention. These are implementation requirements, not claims
that the current service enforces them.

Initial evidence is unadjusted provider-day OHLCV, not regular-session-only or
total-return data. Keep calendar versions and corporate-action observations
explicit. Unknown identity, scope, reference coverage, quality or rights yields
a typed failure. Revised evidence creates a linked immutable snapshot.

Distinguish capture-time evidence from historical knowledge. Freeze only after
all inputs arrive; current retrieval cannot prove past availability. Historical
mode requires demonstrably known-by-cutoff evidence/reference versions. The live
candidate is not qualified for that guarantee and must reject unsupported cases.

Select Massive's documented bars and reference surfaces as the first technical
adapter target. The official-source capability matrix and restrictions live in
the profile. No account tier, credentials, non-display permission, retention or
redistribution entitlement has been approved. Live activation remains blocked
until these are established; inability to qualify requires a replacement decision.

Application-owned ports keep vendor DTOs/SDKs outside domain policy. Bound calls
and retries across restart; keep external I/O outside transactions and finalize
inbox/result/outbox atomically. No alternative workflow engine or model calls are
introduced; [ADR-0013](0013-select-durable-workflow-execution.md) remains unchanged.

## Alternatives considered

### Alpaca as the first adapter

Its documented feed and adjustment controls are viable. Choosing both adapters
adds conformance/licensing work without satisfying a new M03 requirement. IEX
must not substitute for consolidated coverage; defer independent qualification.

### Synthetic-only completion

Safe for public/default CI, but insufficient: the roadmap requires a live
production-candidate provider with matching contracts and failure semantics.

### Implicit adjusted history and presumed account permission

Convenient, but permits look-ahead, incomparable price bases and unauthorized
non-display/storage use. Reject these defaults even when HTTP access succeeds.

## Consequences

### Positive

Later PRs share explicit data meaning, safety boundaries and measurable budgets.
Public demonstrations need no licensed data, credentials or paid services.

### Negative

Raw daily bars cannot answer regular-session-only or adjusted-return requests.
Historical availability and live rights may remain unavailable; those requests
fail rather than silently weakening provenance or authorization.

### Neutral or follow-up

The remaining M03 PRs must implement and test the profile. Vendor account access,
session-scope conformance and rights approval remain activation gates. No current
SLA, legal clearance, live behavior or completed milestone is claimed.

## Compatibility and migration

No runtime, published schema, dependency, database or deployment changes. Existing
M02 conformance remains intact. Future wire/schema versions follow compatibility
policy. Changing supported data meaning requires reviewed profile evolution;
published snapshots and accepted ADR history are not rewritten on rollback.

## Security and operations

No live provider request, vendor contact, credential purchase, model interaction
or brokerage connectivity is authorized. Unknown rights prohibit licensed writes.
The profile requires bounded egress, secret-safe telemetry, scoped encrypted
storage, expiry denial and recoverable physical cleanup before live acceptance.
Code licensing never grants market-data rights.

## Validation

Run `cargo xtask verify`; published ADR identity and navigation are mechanically
checked. Append this record to the immutable ADR ledger/index. Review the dated
official-source matrix and concrete supported/rejected cases. Later offline and
opt-in live qualification must prove semantics, budgets and recovery; document
checks alone cannot establish those behaviors.
