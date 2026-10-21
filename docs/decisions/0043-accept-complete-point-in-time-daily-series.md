# ADR-0043: Accept complete point-in-time daily series

- Status: Accepted
- Date: 2026-10-21
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Validate the entire bounded daily series atomically in the market-data domain.
- Pin explicit quality policy, source semantics, knowledge cutoff, mappings,
  calendar days, provider buckets and complete action-catalog coverage.
- Preserve raw prices; neither splits nor source priority silently repair evidence.
- This decision adds no provider, rights approval, persistence or live activation.

## Context

Canonical observations validate individual values, not whole requested histories.
A structurally valid bar cannot establish completeness, historical availability,
identity continuity or whether an empty action catalog means no actions occurred.
Provider-day buckets differ from exchange core hours, especially on early closes.

## Decision

`DailyQualityPolicy::check` accepts one instrument/listing and an explicit
half-open range of at most 366 calendar days and 252 open sessions. Every date
must resolve through retained calendar history; closed days need explicit coverage.
Every open date must resolve the requested dated alias to the same identities
and have exactly one selected raw provider-day bar matching its explicit bucket.
Buckets contain the reviewed core window, never overlap, use exact millisecond
boundaries and become eligible at bucket end plus the profile's 12-hour grace.
Calendar core close never substitutes for that bucket endpoint.

The caller supplies the exact requested range. For a latest-data request, the
application must first determine the latest eligible session from sufficiently
complete reference/bucket coverage. A passing older range does not establish
global latest freshness. This pure checker does not invent omitted calendar days
or provider semantics. Provider conformance must establish actual bucket meaning.

At most 4096 canonical observations per source enter revision selection. Select
latest known revisions before filtering dates or cancellations. Require retained
predecessors and nondecreasing receipt time. Exact canonical duplicates collapse;
contradictory repetitions or distinct live IDs for one session fail. Receipt time
is part of the immutable canonical observation and is not replaced on redelivery.
Future-known corrections cannot revise an earlier cutoff. Missing predecessors
cannot establish history. A cancellation never revives an older observation.

Mappings, calendar versions, selected actions and an independently versioned
action-completeness attestation must be known by cutoff and within the explicit
maximum reference age. Attested instrument/date coverage must contain the request
and remain valid at cutoff. Ingestion verifies source authority, checksums and
actual completeness; constructing an attestation proves none of those facts.
Unknown reorganizations fail. Actions effective on closed days require unsupported
handling and fail rather than being silently discarded or moved to an open date.
Supported action annotations never modify raw prices.

Policy revision, reference-age ceiling and both basis-point tolerances are
explicit inputs without defaults. Consecutive raw close changes use the preceding
close as denominator. Optional independent sources require the identical evidence
contract and complete coverage; compare each OHLC field using the smaller price
as denominator, symmetrically. Boundaries are inclusive. Exact integer arithmetic
supports the entire existing price envelope without overflow or silent rounding.
Volume is preserved, not equated across providers by this price-comparison policy.
Split metadata does not waive the close-change threshold. Broader or action-aware
acceptance requires an explicit reviewed policy, not implicit adjustment.

Acceptance returns borrowed immutable observations, selected mapping/calendar/action
versions, coverage provenance, requested range and the exact policy/cutoff. It
does not authorize publication, grant data rights or certify capture-only input
as historical evidence. All failures are payload-free and return no partial series.

## Alternatives considered

- Provider-side forward filling or weekday guesses conceal missing facts; reject.
- Adjusted histories conceal original raw values; preserve annotations instead.
- Source ranking, merging and averaging conceal contradictions; require comparison.
- New decimal or calendar dependencies duplicate existing checked domain values.

## Consequences

Acceptance is an additive pure domain capability, not an acquisition service.
Caller-owned immutable inputs outlive the borrowed result. Whole-series failure
prevents downstream users from mistaking a partial history for complete evidence.
Explicit freshness/outlier policy may reject otherwise structurally valid inputs.

## Compatibility and migration

No dependency, database schema, event format or deployable changes. The additive
domain API can be reverted without migration. No existing observations change.
Later acquisition contracts must apply entitlement, capability and latest-request
selection checks before publishing.

## Security and operations

Ingestion must authenticate coverage authority, rights, receipt timestamps and
source checksums. Domain constructors cannot grant those capabilities. Public
fixtures contain invented data only; diagnostics omit financial payloads.
There is no external I/O, background task, broker route or model operation.

## Validation

The default `market-data-quality` harness check protects six named deterministic
proofs. Synthetic fixtures exercise closures, DST, early closes, grace/freshness
boundaries, missing/conflicting/revised evidence, cancellation, action authority
boundaries, outliers and independent sources. No licensed data or wall clock enters.

Run the focused check and `cargo xtask verify`; neither establishes live-provider
qualification, operational availability, data licensing or market-data realism.
