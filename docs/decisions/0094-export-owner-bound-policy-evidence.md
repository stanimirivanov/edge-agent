# ADR-0094: Export owner-bound policy evidence

- Status: Accepted
- Date: 2026-10-30
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Export an additive companion with exact daily evidence and explicit event coverage.
- Use existing owner authorization and retention; unknown never means no events.
- Qualify affirmative coverage through a dedicated invented recipe only.

## Context

M05 policy needs retained OHLC/volume and event coverage. Research currently reduces
daily evidence to closes. Existing recipes assert no complete event calendar.

## Decision

Keep daily artifacts unchanged and add an opt-in canonical policy companion. Bind
original request, daily digest, source/reference lineage and point-in-time knowledge.
Use the existing owner read path and a bounded Research process adapter; a hash alone
is no authority. Reject future knowledge, incompatible scope and expired retention.

The maintainer selected a dedicated policy-history-v1 synthetic recipe. Pin its empty
invented event calendar specification into its configuration fingerprint. All older
recipes export unknown coverage. Freeze earlier history source bytes to preserve
retained admissions, following the existing single-session compatibility pattern.

## Alternatives considered

Treating absent events as complete would invent evidence. Operator-installed calendars
would add another authority/retention path. Rewriting daily objects would invalidate
retained receipts and hashes. A live event provider is outside this slice.

## Consequences

Later liquidity, gap and event policy can consume explicit owner-bound inputs without
querying private tables. No strategy eligibility or policy approval is granted here.
Consumers must still verify the coverage interval contains their full required window.

## Compatibility and migration

No existing payload, database migration or broker route changes. Deploy owner-reader
support first. Legacy recipe fingerprints use frozen source bytes. Old binaries cannot
restore new policy-history acquisitions; retain a compatible binary or quiesce before
rollback. Full MarketData event-provider scope remains with M03.

## Security and operations

Current rights are rechecked on owner reads and Research retention fences remain.
Synthetic completeness is explicitly labeled and does not assert live-provider facts.
No credentials, arbitrary fetch locations, model output or new dependency is introduced.

## Validation

Contract/loader tests cover exact binding, future coverage, interval/census bounds,
unknown fields and frozen history fingerprint. Real owner processes compare daily
bytes, distinguish unknown/complete coverage, repeat reads and refuse revoked rights.
Run repository/architecture sensors and the full default gate.
