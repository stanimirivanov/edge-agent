# ADR-0057: Capture native daily provider responses under durable permits

- Status: Superseded
- Date: 2026-10-24
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by: ADR-0058

## TL;DR

Implement Twelve Data's daily protocol without live activation. Preserve native
dates in separate bounded capture objects. Meter requests through the existing
durable ledger. Precise daily-v1 evidence and its publisher stay unchanged.

## Context

ADR-0056 supplies temporal provenance, not a full observation/object path. Promoting
a date to a precise source instant invents availability. Native responses also
need exact parsing, selection, rights, quota accounting and complete calendars.
Invented protocol fixtures cannot qualify a real account, dataset or data license.

## Decision

Add `TimestampPrecision::DateOnly`, allowed only for raw provider-day bars. It
accepts no precise instant. Native admissions use the independent
`edgeagent.admission.date-only.v1`; old v1/v2 meanings remain unchanged. Bind exact
selection and reviewed bucket/calendar inputs into the native configuration hash.

The application owns complete `DateOnlySchedule` validation: every requested date,
including closed days, exactly one reviewed bucket per open day, at most 252 open
rows, completed buckets, exact listing and native capture-only semantics. Check
current authority and rights embargo before and after I/O. Reference authentication
and source-checksum verification remain owner obligations, not vendor assertions.

Fix HTTPS origin/path. Request one selected symbol/MIC, `interval=1day`, explicit
`adjust=none`, ascending order, explicit date bounds/output size. Send only a
sensitive `Authorization: apikey` header. No redirects, query credentials, hidden
pagination, sleeps or retries. Existing public bearer behavior stays unchanged.

Reuse durable call permits, endpoint-weighted daily credits, account spacing,
byte/time ceilings and cancellation. A session fetches once. If accounting
confirmation is lost after receipt, retry confirmation only, preserving original
receipt/body hash. Unknown calls remain charged and fenced.

Seal exact USD/share OHLCV and native labels as
`edgeagent.evidence.date-only-daily-capture.v1`, with admission scope/digest, feed
semantics, configuration/response hashes and dated temporal witnesses. Require
sorted JSON+LF, SHA-256, at most 252 rows/4 MiB, strict order and exact decoding.
`DailyObject` explicitly dispatches native captures versus precise quality-approved
artifacts. Provide no implicit conversion or daily-v1 publication path.

## Alternatives considered

- Midnight/core-close promotion invents an event and false availability.
- Optional daily-v1 fields change a published format's meaning.
- Vendor SDK/parallel limiter duplicates bounded transport and durable accounting.
- Automatic pagination/retry hides billable work and uncertain commit outcomes.
- Enabling the existing worker bypasses its precise-only quality/publication contract.

## Consequences

PR11E reuses the native schedule/object and consumer dispatch. Captures are not
whole-series quality acceptance or `EvidenceDecision::ready`. Before PR15, add
native quality, scoped storage and terminal publication integration, authenticated
runtime authority/fencing and account-specific approval. Installing an adapter
does not enable it. The offline worker stays unchanged.

## Compatibility and migration

Existing schemas/fixtures, precise admission-v1/v2, database schema and terminal
messages remain unchanged. Old readers reject the separate native format. Append
new immutable schema/golden identities. No deployment or database migration;
runtime activation and native storage/publication need separate reviewed work.

## Security and operations

Consult public documentation only. No provider calls, credentials, licensed data,
SDK dependencies, model runtimes or broker capabilities. Redact request/payload
diagnostics. Public pricing is not entitlement; allocation, opening debit, expiry
and endpoint price remain owner-reviewed. Classify HTTP-200 errors without message
text. Actual coverage, availability and rights still require qualification.

## Validation

Protected `twelve-data-conformance` loopback proofs cover outbound binding,
header-only authentication, exact native captures, malformed/foreign/incomplete
responses, errors, cancellation, denied authority, charging and confirmation loss
without refetch. Independent census mutations protect discovery. Evidence tests
retain all published baselines. Default `cargo xtask verify` remains offline.
