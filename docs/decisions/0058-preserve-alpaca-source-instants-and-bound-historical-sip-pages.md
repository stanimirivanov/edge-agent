# ADR-0058: Preserve Alpaca source instants and bound historical SIP pages

- Status: Accepted
- Date: 2026-10-25
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes: ADR-0057
- Superseded by:

## TL;DR

Retain ADR-0057's entire Twelve Data/native-date decision by reference, but replace
its assumption that Alpaca also uses date-only captures. Preserve Alpaca's reported
bar-opening instants in precise canonical observations. Every explicit historical
SIP page requires a durable permit. Neither adapter activates a worker.

## Context

Alpaca's historical-bars examples report RFC3339 opening timestamps. Discarding them
into date-only evidence loses source precision. The maintainer approved preserving
them and correcting the PR11E plan on 2026-10-25. The API can paginate below its
requested limit; defaults can select IEX, remap symbols or include restricted recent
data. Public documentation and synthetic proofs cannot qualify an account or rights.

## Decision

All Twelve Data behavior, formats, authority, quota, security and activation rules
from ADR-0057 remain normative. Only its Alpaca reuse assumption changes. Never
erase Alpaca timestamps into `DateOnlyTime`. Existing published evidence stays intact.

The application owns `PreciseDailySchedule`: selected instrument/listing, complete
reviewed dated calendars including closed days, explicit nonoverlapping provider
buckets, owner observation identities/revisions and transformation revision. Require
capture-only consolidated raw daily semantics, nanosecond-capable precision, known
references and complete bounded rows. Bind values and witnesses with versioned,
fixed-width/length-delimited encoding, not Debug text. Invent no observation lineage.

Use only `https://data.alpaca.markets/v2/stocks/bars`, one selected symbol,
`timeframe=1Day`, `feed=sip`, `adjustment=raw`, `currency=USD`, `sort=asc`, `asof=-`,
explicit inclusive opening-time bounds and `limit=252`. Disable vendor symbol-history
stitching; require the dated mapping to cover the range. Every completed bucket must
be at least 15 minutes old and satisfy stricter current rights embargoes. No IEX fallback.

Send paired sensitive APCA headers, never query credentials, Basic auth or an order
endpoint. Constructors cannot establish vendor-side credential scope: acceptable
principals remain an activation prerequisite. Reuse existing TLS, redirect/encoding
restrictions, original deadlines, durable credits, account spacing and byte/call
limits. Each caller-requested page gets a fresh permit; no automatic retry or sleep.

Keep tokens opaque, bounded and URL-encoded under the sole allowed query key. Reject
cycles, foreign symbols, malformed/null critical fields, duplicate JSON keys,
unexpected/reordered times and incomplete final series. Release no partial batch.
Known pre-dispatch rate rejection preserves the page without a charge; uncertainty
stays fenced. Confirmation retries recover received pages without HTTP, preserving
original receipt and body hash. `DailySourceCapture` is canonical input plus page
provenance, not quality acceptance, a wire object or permission to store/publish.

Use the existing pinned Chrono parser, not a handwritten RFC3339 parser or SDK.
Support explicit UTC timestamps with at most nine fraction digits; reject date-only,
non-UTC, leap-second and excess-fraction variants without truncation. Reuse current
`validate_batch` and complete daily quality before later freezing/publication.

## Alternatives considered

- Literal native-date reuse discards source precision.
- IEX/subscription defaults change coverage and volume semantics.
- Default `asof` stitches renamed symbols beyond reviewed mapping scope.
- Automatic paging/waits hide billable work and uncertainty from the owner.
- Handwritten parsing or vendor SDKs add parser risk or brokerage surface.

## Consequences

PR11F remains the Twelve Data native-date quality/storage/publication prerequisite.
Alpaca reuses precise canonical evidence, but neither adapter enters the current
offline-only worker. PR15 still needs runtime integration, authenticated catalogs
and fencing, reviewed account rights/allocations and live qualification. Requests
alone cannot prove the provider honored SIP, USD, raw basis, entity or bucket rules.

## Compatibility and migration

No schema, message, SQL or storage migration. Precise admission-v2 pins the new
configuration. Existing precise/native artifacts remain unchanged. Chrono already
exists in the pinned workspace resolution; add only its direct HTTP dependency,
with no new registry packages or versions. Reverting this inactive adapter leaves
the offline worker intact; never rewrite retained evidence to another time model.

## Security and operations

Public documentation and invented loopback payloads only: no credentials, account
access, provider calls, purchases, models or brokerage capabilities. Diagnostics
redact configuration, tokens, bodies and headers. Advertised free tiers or library
licenses are not data rights. Explicit exclusive allocation, opening usage and
endpoint price remain owner-reviewed. Do not raise local four-starts/minute or
15-second spacing ceilings to advertised provider rates.

## Validation

Protected `alpaca-conformance` proofs cover exact SIP/raw/asof/header binding,
nanosecond times and prices beyond f64 precision, permit-bound pages, confirmation
recovery, rate rejection, malformed/foreign/duplicate/cyclic pages, missing
authority/credentials, spent credits and no IEX fallback. Independent census
mutations protect discovery. Run `cargo xtask verify`, dependency policy and
available supply-chain stages; missing SBOM tools remain not-run, not passing.
