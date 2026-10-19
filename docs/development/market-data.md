# Market-data evidence profile

## TL;DR

- `us-equity-daily-v1` targets bounded, USD-denominated U.S. equity/ETF evidence.
- Provider selection is deferred; no vendor or data license is committed.
- Default tests and public demonstrations use synthetic, redistribution-safe data.
- Historical prices fetched now establish capture-time evidence, not historical
  knowledge. Unknown rights, calendar coverage or provenance fail closed.
- This profile specifies future M03 implementation; it adds no working adapter,
  configuration key, live qualification command or production-data permission.

## Authority and supported inputs

[ADR-0032](../decisions/0032-define-vendor-neutral-simulation-profiles.md)
retains this daily evidence meaning and replaces ADR-0031's vendor selection.
The [research/simulation profiles](simulation-profiles.md) define separate
planned intraday/options capabilities, not changes to daily-v1. The
[M03 roadmap](../roadmap/milestones.md#m03---market-data-fabric)
owns completion; the [product vision](../product/vision.md) owns product scope.
Normative terms follow [CONTRIBUTING](../../CONTRIBUTING.md#policy-language-and-sources-of-truth).

| Dimension | `us-equity-daily-v1` requirement |
|---|---|
| Universe | Explicit, versioned allowlist of liquid common equities and unlevered broad/sector ETFs listed on XNYS, XNAS or ARCX; USD only. Admission is not a claim that a security is currently liquid. |
| Identity | Service-owned instrument and listing IDs; provider symbol, venue, security type, currency and effective mapping interval are attributes, never the primary identity. Reject ambiguous symbol reuse. |
| Request | One instrument, at most 252 expected sessions within 366 calendar days. The 3–20-session research horizon does not define acquisition lookback. |
| Bar scope | Explicit provider-day aggregate and provider trade-eligibility revision; never label it regular-session-only. Core-session-only, intraday and overnight reconstruction are unsupported. |
| Basis | Unadjusted OHLC and reported share volume only. Request `adjusted=false` explicitly; reject a contradictory response. No dividend, split-adjusted or total-return series in v1. |
| Numbers | Parse numeric tokens exactly, not through binary floating point. Maximum 28 significant digits, price scale 8 and share-volume scale 6; no silent rounding. OHLC must be positive and internally ordered; volume nonnegative. |
| Time | UTC instants plus exchange-local session date and `America/New_York` timezone rules. Preserve the provider aggregate timestamp separately; it is not a receipt or availability timestamp. |
| Exclusions | OTC, options, crypto, non-USD, leveraged/inverse products, unsupported listings, automatic universe discovery, fabricated missing bars and live trading. |

The adapter MUST establish the aggregate's actual interval and eligible-session
coverage before live qualification. A calendar session and a provider-day bucket
are different concepts. Unknown or changed vendor session semantics require a
typed unsupported result, not relabeling. Future trading-hours changes require
new reviewed reference coverage, not a perpetual weekday rule.

## Point-in-time and reference authority

The [domain crate](../../crates/market-data-domain/src/lib.rs) now implements
checked instrument/listing/source identities, transformation revisions, USD
prices, reported share volume, Gregorian dates, UTC millisecond instants and
causally ordered observation times. Fields are private and values carry no wire
format, provider SDK or persistence type. Decimal constructors take exact signed
coefficient/scale pairs, reject excess scale without rounding, and normalize
fractional trailing zeros before the 28-digit coefficient check. Equality,
ordering and hashing are numeric; `decimal_parts()` round-trips losslessly.
Price/volume diagnostics are redacted. Missing provider timestamps remain absent.
The bounded integer representation implements no decimal parser or arithmetic
engine; adapters must parse source tokens exactly before constructing values.
Dates alone prove neither session membership nor timezone conversion; source
IDs/revisions alone prove neither entitlement nor historical availability.
Provider token parsing and wire DTOs remain later M03 slices; this library does
not make the bootstrap service acquire evidence.

The domain also implements immutable `ReferenceVersion<T>` observations and
bounded `ReferenceHistory<T>` selection for mappings, calendar days and actions.
Each version records stable record ID, increasing reference revision, source,
source-byte SHA-256 and actual local receipt/knowledge time. Histories accept at
most 4096 versions; duplicate revisions and decreasing receipt times fail.
Corrections reuse the logical record ID. Selection first chooses its latest
revision known at the inclusive cutoff, then filters effective dates. Filtering
effective dates first would resurrect superseded mappings or actions. Missing
retained history cannot establish earlier availability.

Mapping intervals are half-open and bounded; ticker reuse preserves distinct
instrument/listing identities. Missing mappings and overlapping current records
return typed failures, never source-priority guesses. Calendar coverage consists
only of explicitly supplied days, including closed days and early closes. Every
day pins `America/New_York`, a named timezone-data revision and reviewed UTC core
session bounds; absent dates fail, including weekends. Constructors check ordering
and same-date bounds, not exchange authority or correctness of DST conversion.
Ingestion must verify source checksums, actual receipt times, timezone rules and
closure notices before admitting these records. The invented
[reference fixtures](../../crates/market-data-domain/tests/fixtures/mod.rs) and
[regression suite](../../crates/market-data-domain/tests/reference.rs) exercise
cutoffs, revisions, ticker reuse, holidays, early closes and UTC shifts across
DST; they are not production calendar/reference coverage.

Corporate-action metadata supports exact positive old/new split share counts,
positive USD cash dividends, explicit unsupported actions and append-only
cancellation revisions. Its effective date means split execution or dividend
ex-date, not announcement/receipt. These annotations never adjust raw prices or
splice identities. Unknown reorganizations must remain explicit unsupported
records. An empty action lookup does not prove completeness; provider/reference
conformance must establish coverage before downstream evidence acceptance.

Capture mode freezes a snapshot only after every input has been received and
validated. Its knowledge time is at least the latest input receipt time. Preserve
event/effective time, provider-declared publication time when available, local
receipt time, immutable request/admission identity and transformation revision.
A provider request ID or response checksum is provenance, not a revision history.

Historical mode requires previously captured immutable evidence and reference
versions demonstrably known by the requested cutoff, or a provider capability
proving that history. A date filter, current adjusted history, or an action's
effective date alone cannot establish historical knowledge. No live provider
has been qualified for that capability; otherwise return a typed
historical-availability failure. Never backdate current acquisition.

Reference authority is a bounded, reviewed catalog, not the current ticker list:

- Instrument mappings use dated provider reference observations, with source,
  checksum, effective interval and receipt/knowledge time. Missing stable mapping
  evidence fails closed; current listings cannot establish past universe membership.
- Calendar versions derive from the official [NYSE schedule](https://www.nyse.com/trade/hours-calendars)
  and [Nasdaq schedule](https://www.nasdaq.com/market-activity/stock-market-holiday-schedule).
  Record supported dates, venue, timezone-data revision, holidays, early closes
  and reviewed closure notices. Unknown dates or conflicting notices fail closed.
- Splits and cash-dividend observations use provider reference data with effective
  dates and first-known times. They annotate raw evidence; they do not authorize
  adjusted prices or reinvestment calculations. Unsupported reorganizations or
  unresolved identity continuity fail rather than splice histories.

For latest-data requests, v1 allows an availability grace of 12 hours after the
provider-day bucket ends. Require the latest expected session whose bucket and
grace have elapsed, using the pinned calendar. Earlier sessions remain required
inside an explicit requested range. Grace is local policy, not a provider SLA or
proof of finality. Missing eligible sessions, stale references, contradictory
duplicates and unapproved outliers produce typed failures; no forward filling.
Publish corrections as new linked snapshots, never overwrite old evidence.

## Historical provider capability review

Official sources reviewed on 2026-10-18; account-specific access remains unverified.
This prior comparison is retained as research, not a current selection or a
fresh assessment of intraday/options coverage. ADR-0032 defers provider choice.

| Candidate/capability | Evidence and disposition |
|---|---|
| Massive daily bars | [Custom bars](https://massive.com/docs/rest/stocks/aggregates/custom-bars) documents OHLCV, split-adjustment control and pagination. Prior candidate only; bounds, session scope and account access still require conformance. |
| Massive identity | [Ticker overview](https://massive.com/docs/rest/stocks/tickers/ticker-overview) supports dated lookup and external identifiers. It informs mappings; it does not replace service identity or prove historical knowledge. |
| Massive actions | [Splits](https://massive.com/docs/rest/stocks/corporate-actions/splits) and [dividends](https://massive.com/docs/rest/stocks/corporate-actions/dividends) supply reference observations. Pin the implemented endpoint revision; do not infer announcement-time history. |
| Massive calendar | [Market holidays](https://massive.com/docs/rest/stocks/market-operations/market-holidays) is supplemental, not sufficient proof of a complete historical calendar. Exchange schedules remain authoritative. |
| Massive rights | [Market-data terms](https://www.massive.com/legal/market-data-terms-of-service) restrict non-display use, derived works and redistribution unless authorized. Default individual access is NOT treated as permission for this pipeline. |
| Alpaca alternative | [Market-data plans](https://docs.alpaca.markets/us/docs/about-market-data-api) distinguish feeds; [historical bars](https://docs.alpaca.markets/us/reference/stockbars) expose feed/adjustment choices. IEX is not interchangeable with consolidated coverage. Not selected; SIP and applicable rights require separate qualification. |
| Synthetic only | Selected for public/default verification, rejected as the complete milestone: M03 also requires a qualified live provider. |

Select a provider later through capability conformance and verified rights;
neither an earlier comparison nor API access establishes entitlement, an SLA or
semantic equivalence. Missing rights or semantics block live activation. No
subscription purchase, vendor contact or credential submission is authorized by
this profile. Data-only credentials MUST NOT enable broker connectivity.

## Entitlements, retention and safe evidence

Live acquisition is disabled until an operator supplies a reviewed rights record:
principal/account and agreement reference, allowed endpoints/instruments/feed,
automated non-display processing and derived-evidence permission, permitted
storage locations, retention/deletion deadlines, audience/redistribution scope,
effective/expiry times and revocation policy. Store references, never agreements
or credentials, in public evidence. Unknown, expired or conflicting rights deny
acquisition and reads. An API key, free plan or open-source code license is not
a data license. This is an engineering activation gate, not legal advice.

Raw response retention is off by default. Canonical evidence has a local maximum
of 24 hours after capture, shortened by the rights expiry/deletion deadline; no
positive entitlement means no licensed storage. Explicitly permitted raw storage
uses the same or shorter deadline. Qualification uses at most 30 minutes and
awaits owned-resource cleanup. No production/backup store is authorized here.

Expired or revoked data is immediately unavailable even while deletion retries.
Enforce scoped physical deletion and orphan cleanup in M03-PR12; metadata TTL is
not deletion. Encrypted storage and scoped identities are required for live
evidence. Do not cross-license deduplicate physical objects merely because hashes
match. SHA-256 hashes cover canonical bytes under a named serialization revision;
record schema, policy, provider and reference revisions. An object ETag is not a
checksum. Public fixtures MUST be invented,
not lightly edited vendor samples. Logs/CI artifacts exclude payloads, secrets,
licensed prices and unapproved derived values, including hashes where restricted.

## Acquisition and resource budget

These are future application-enforced ceilings, not currently implemented settings
or provider-plan promises. Lower account limits always win; increases require a
reviewed profile revision. Counts include pagination, references and failed calls.

| Resource | v1 ceiling |
|---|---|
| Active work | Two admitted acquisitions per worker; one external request in flight per provider account across replicas. |
| Rate | At most four request starts per rolling minute, at least 15 seconds apart, or the lower verified account limit. |
| Retry budget | Three acquisition attempts and 12 external requests total per immutable command, persisted across restarts. No retry multiplication across layers. |
| Time | 5-second connect and 15-second whole-request deadline; 300-second command deadline from first durable admission, including waits and retries. |
| Pagination | At most four pages per endpoint traversal; same authorized HTTPS origin only. A next-page link cannot expand authority. |
| Bytes | 1 MiB decoded per response, 8 MiB across the command, 4 MiB canonical evidence object; bounded streaming/decompression. Existing 256 KiB event-envelope limit still applies. |

Only classified transient failures are retryable. Honor `Retry-After` within the
remaining persisted budget; if it cannot fit, terminate with an explicit budget
failure. Cancellation/deadline stops new work, but cannot prove a remote request
was undone. Rights, invalid input, unsupported semantics and quality failures are
terminal. Perform external I/O outside database transactions; commit the terminal
receipt/inbox/outbox atomically, then ACK. Recover uncertain effects by immutable
identity and checksum. A committed receipt skips acquisition; an interrupted
provider read may repeat and consumes the same persisted request budget. This
does not promise exactly-once remote execution. This bounded consumer is not a
custom durable workflow engine; [ADR-0013](../decisions/0013-select-durable-workflow-execution.md)
still owns long-running workflow technology.

## Qualification and delivery boundaries

The existing `cargo xtask verify` verifies documentation/governance now; it does
NOT establish provider behavior or grant rights. There is no live market-data
qualification command today. Later PRs MUST add offline adapter conformance,
owned storage/process recovery proofs and a separately authorized opt-in live
gate. Missing credentials, rights or prerequisites must report not-run/nonzero,
never a passing live qualification. Fork/default CI receives no live secrets.

M03-PR2–PR6 implement domain/reference/quality/provider/message contracts;
PR7–PR10 implement storage and bounded acquisition; PR11 implements a later
qualified adapter; PR12 retention; PR13 runtime; PR14 offline recovery; PR15 live acceptance.
Synthetic and live results share contracts and failure semantics, not prices.
M03 stays incomplete until live qualification passes under verified rights.
No models, adjusted-return analytics, UI, cloud rollout or trade routing belong
to this profile. Published evidence remains immutable on correction or rollback.
