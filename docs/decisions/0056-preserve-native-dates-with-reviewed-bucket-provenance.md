# ADR-0056: Preserve native dates with reviewed bucket provenance

- Status: Accepted
- Date: 2026-10-24
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Keep a provider date distinct from a source instant. Add an independent, bounded
temporal-value contract with explicit reviewed UTC bucket derivation, rule/calendar
provenance and actual local receipt. Existing daily-v1 evidence remains unchanged.

## Context

The conditional daily adapters need to preserve native YYYY-MM-DD labels. Existing
`EvidenceTimes` and `TimestampPrecision` model precise source instants. Converting
a date to midnight, bucket start or close would invent source precision and could
misstate availability. A reviewed exchange calendar is also not a provider bucket.

## Decision

Add a separate `DateOnlyTime` domain value. Its owner supplies the original label,
an immutable `ReferenceVersion<DateOnlyBucket>`, exact calendar version, actual
local receipt and actual local derivation time. A bucket pins its local date,
explicit UTC interval and calendar revision. Bounds remain millisecond-aligned
and at most 26 hours; this capacity allows explicitly supplied 23/25-hour intervals,
not automatic timezone or session inference.

Check label/calendar/revision equality, explicit open or early-close classification,
core-session coverage inside the provider bucket, receipt at/after bucket end and
derivation at/after receipt and reference knowledge. References may be received
after the payload; the derived value must not claim those inputs were known earlier.
Authentication, source-checksum verification, reference completeness and the
correctness of provider aggregation rules remain trusted producer obligations.

The pure application mapper seals `edgeagent.evidence.date-only-time.v1`. This is
a reusable temporal value, not a new publication route or full daily object. It
retains source date, explicit bucket, actual receipt/derivation, exact rule and
calendar witnesses, source checksums, America/New_York and named tzdata revision.
Wire validation checks shape/chronology/pins, not the retained calendar's omitted
core hours: domain construction performs that check. Retain and authenticate the
complete referenced records. A hash/version is not entitlement or truth.

Seal recursively key-sorted JSON with one LF and SHA-256, capped at 4096 bytes.
Bound before parsing; reject wrong digests, unsupported versions, unknown/missing
fields, duplicate keys and alternate encodings without payload diagnostics. Use
a separate local Draft 2020-12 schema and immutable synthetic golden baseline.
Provide no `event_at`, inferred publication time or conversion to `EvidenceTimes`.

## Alternatives considered

- Midnight/core-close promotion: invents a precise source event and conflates scopes.
- Add optional date provenance to daily-v1: changes a published format's semantics.
- Store only a label: loses which rule/calendar produced the UTC bucket.
- Infer historical knowledge from the label: contradicts actual capture-time evidence.
- Implement vendor/runtime/publication simultaneously: obscures a reusable contract
  decision and couples it to unqualified credentials, rights and feed semantics.

## Consequences

Future date-only observations and full object formats must embed this value and
retain its provenance. They cannot pass through the precise-time daily-v1 path.
PR11D must implement that integration explicitly, not claim this prerequisite
activates an adapter or makes existing publication/storage accept date-only bars.
The type supports the conditional first and second daily adapters without vendor
SDKs, new dependencies, clock reads or timezone extrapolation.

## Compatibility and migration

Existing daily-v1 DTOs, schemas, fixture bytes, digests, message definitions,
admissions and storage/publication APIs are unchanged. The new schema/value has
its own format and URI; old readers reject it. No database migration, rewrite,
new service configuration or rollout is required. Reverting this unused runtime
foundation requires no data deletion. Before later activation, implement a new
parent observation/object format and strict consumer format dispatch.

## Security and operations

No provider, credential, model, broker or cloud call occurs. Synthetic DST/early
close examples are invented arithmetic fixtures, not qualified exchange calendars.
This value contains no market price or bearer location and grants no data access.
Diagnostics redact DTOs/bytes; explicit source access remains owner-controlled.
There is no live acquisition, automatic retry or permission to activate PR15.

## Validation

Protected offline `date-only-evidence` proofs cover label/revision/calendar
conflicts, closed days, future-known inputs, explicit 23/25-hour and early-close
buckets, receipt precision and strict bounded decoding. Independent harness census
mutations reject removal/rename/ignore/extra tests. Evidence schema/golden tests
preserve all published v1 hashes and append the new schema/value identities.
Workspace doctests reject conversion to precise `EvidenceTimes` at compile time.
