# ADR-0071: Pin IEX canonical interpretation to complete owner profiles

- Status: Accepted
- Date: 2026-10-27
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Require exact owner-selected conditions, references, windows and identity partitions.
- Commit complete profile values, not just IDs/checksums, with a versioned encoding.
- Preserve snapshot delivery, native metadata and original source/receipt times.
- Add no runtime, independent approval, publication or readiness grant in PR20C1.

## Context

The IEX protocol adapter preserves raw source metadata but cannot infer trade
eligibility, exchange-local sessions or global logical identity. PR20A supplies
transient scheduling and PR20B supplies independent restore control. The maintainer
approved splitting canonical interpretation from actual runtime composition.

## Decision

Keep provider-specific interpretation in `market-data-http`, depending inward on
existing canonical domain values. Freeze an explicit snapshot IEX trade OR raw
minute-bar profile. Bind source, contract/semantic reference, transformation,
complete mapping/calendar records, explicit source-time windows, exact condition
rules and namespaces into deterministic array-only versioned bytes. Cap profile
cardinality and bytes; order-independent lane/rule collections sort canonically.
Reject ambiguous partitions, namespaces and source-condition sets.

Require the profile digest to match the sealed session configuration and complete
selection witnesses. Resolve fresh exact mapping/calendar histories at the supplied
evaluation instant; require pinned references to have been known at original receipt.
Reject unknown codes, missing explicit empty-condition rules, incompatible contracts,
closed days, changed references, cancellation and invalid source/receipt/capture bounds.
Do not invent a timezone conversion, actual exchange calendar or condition eligibility.
The independently reviewing owner supplies those meanings and allocates namespaces.

Logical identity uses a nonzero owner-reserved high 64-bit partition and exact
source trade ID or nonnegative Unix minute bucket in the low 64 bits. A partition
names one source/instrument/date/kind and, for trades, exchange/tape. Profiles enforce
local uniqueness only; cross-profile reservations remain an owner/runtime obligation.
Do not use arrival ordinals, local receipt or random IDs to distinguish replayed facts.
Emit revision 1 without a sequence. Corrections/updated bars still require durable
reconciliation, not guessed revisions. A changed immutable fact must fail downstream
continuity checks rather than silently replace history.

Return canonical observations together with original normalized native metadata and
profile commitment. Retain trade IDs/conditions/exchange/tape, bar count/VWAP and
frame/item positions. Canonical values alone cannot represent all that provenance.

## Alternatives considered

- Built-in condition/venue/session defaults: invents provider eligibility or rights.
- Pin only IDs/checksums: changed complete values can retain the same supplied labels.
- IDs from receipt/order/float conversions: reconnects change identity or lose precision.
- Implicit minute-bar finality or source continuity: IEX does not provide those guarantees.
- Treat digest consistency as approval: a caller can construct a matching session locally.

## Consequences

PR20C1 is a deterministic interpretation library, not proof of an authenticated owner.
PR20C2 must authenticate full inputs against the independently installed admission,
maintain namespace ownership across profiles, persist native/profile provenance and
compose restore fences, publication, bounded tasks and existing domain readiness.
Snapshot capture does not qualify historical availability, freshness or a core session.
Owner windows may explicitly cover extended hours; raw bars stay raw intraday bars.

## Compatibility and migration

Additive Rust API and private `edgeagent.iex-canonical-profile.v1` configuration bytes.
Only users of this acceptor must bind this digest as their session configuration;
other session meanings and published formats remain unchanged. No schema, dependency
or runtime selector changes. Stop using the acceptor to roll back; do not rewrite
existing admissions, logical IDs, retained evidence or unresolved resource counters.

## Security and operations

Bound cardinality/tokens before copying and preserve lossless time/numeric values.
Redact diagnostics for profiles, references, rules and accepted native/canonical data.
Catalog authentication and verified source checksums remain trusted owner obligations;
fresh history comparison detects corrections, not dishonest catalog construction.
No secret, provider request, model call, system clock or live activation is introduced.

## Validation

Protected `iex-canonical-acceptance` proofs use invented references, condition rules
and observations, including JSON object-order/compression feature unification.
Local compilation and strict Clippy are required; heavier proofs run in CI.
See [IEX canonical acceptance](../development/iex-canonical-acceptance.md).
