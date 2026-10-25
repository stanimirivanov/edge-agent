# Alpaca historical daily protocol

## TL;DR

- Offline-conformant historical SIP protocol, not live activation.
- Preserve RFC3339 source opening instants; never replace them with dates.
- Require complete reviewed schedules, owner observation IDs and durable credits.
- Request each page explicitly; confirmation retries never refetch received pages.
- Run `cargo xtask verify --check alpaca-conformance`, then `cargo xtask verify`.

## Inputs and authority

Resolve `SelectionPlan::prepare`, then construct `PreciseDailySchedule` with every
dated calendar including closed days, explicit provider-day intervals and one
`DailyObservationSlot` per open day. Supply owner observation lineage and a
transformation revision; the adapter invents neither IDs nor historical knowledge.
Maximum 366 calendar days/252 open rows. Core hours fit inside explicit
nonoverlapping buckets, not replace them. Authenticate/retain the full reference
records and source bytes outside this API; references must be known at admission.

The reviewed contract is capture-only consolidated raw daily data with
nanosecond-capable precision. Preserve supplied instants, including zero fractions;
this does not claim a provider always emits nine digits. Pin
`schedule.configuration_digest()` via `AdmissionRecord::with_configuration`.
Configuration includes selection, calendars, actual intervals, IDs/revisions and
transformation, not only caller-supplied checksums or unstable Debug output.

Configure the existing ledger with reviewed exclusive account allocation, opening
usage, expiry and maximum endpoint price. Key count is not quota. Keep local four
starts/minute and 15-second spacing; never guess free-tier entitlements. Recheck
current rights before dispatch and release. Unknown/withdrawn/changed authority denies.

## Protocol and explicit paging

The fixed endpoint is `https://data.alpaca.markets/v2/stocks/bars`. Use paired
sensitive APCA headers, never URL credentials, Basic auth or brokerage endpoints.
Only private tests permit loopback HTTP; no production endpoint/feed override.
Constructors cannot verify vendor-side key scope. Acceptable restricted principals
remain an explicit activation decision; unavailable scope keeps activation blocked.

Send one selected alias, `timeframe=1Day`, `feed=sip`, `adjustment=raw`, `currency=USD`,
`sort=asc`, `asof=-`, `limit=252` and explicit inclusive first/last expected opening
instants. Disable implicit alias-history stitching; require the selected mapping
to cover the range. Every bucket must be completed for at least 900000 ms, or a
stricter rights embargo. Recent-data denial never falls back to IEX. Responses do
not echo all request semantics: qualification must establish actual feed, currency,
security type, entity identity, unadjusted basis and provider aggregation coverage.

`fetch_page` dispatches one permit-bound page and returns `More` or `Complete`.
The caller honors durable rate rejection; no auto-sleep/retry. Only known
pre-dispatch rate denial leaves a page ready. Unknown reservation/request work stays
fenced. After lost accounting confirmation use `confirm_page`, which sends no HTTP
and preserves original receipt and body hash. Dropped futures do not undo calls.

Treat `next_page_token` as opaque data, not a URL. Encode it under the sole allowed
query key, preserving fixed parameters. Tokens are at most 1024 ASCII graphic bytes;
reject cycles and enforce command page/call/byte/deadline ceilings. Missing token
fields are not explicit null. Short pages still need continuation checks. Complete
output must contain every expected timestamp once, in order. Missing, duplicate,
foreign or unexpected rows fail; no partial batch or fabricated bars escape.

Prices/volume borrow exact JSON tokens, never floats. Use the pinned Chrono RFC3339
parser and checked nanosecond values for source `t`. Support explicit `T`/`Z`, at
most nine fraction digits and no leap seconds. Dates and offset variants are
unsupported, not repaired or truncated. Receipt uses the trusted local transport
clock and survives confirmation delay independently from the provider timestamp.

## Capture and remaining qualification

`DailySourceCapture` exposes unapproved `ProviderBatch` inputs and ordered exact
page SHA-256/receipt provenance. It implements no wire/storage/publication conversion.
Recheck `validate_batch`, references/actions and whole-series quality before freezing.
Current receipts cannot establish historical availability. Raw retention requires
separate permission; do not log provider bytes or automatically print error causes.

The offline worker remains unchanged. PR11F owns native-date integration for Twelve
Data; Alpaca does not need that lossy detour. PR15 still needs runtime composition,
authenticated catalogs/fencing, reviewed rights/account limits and authorized live
qualification. Invented proofs do not certify a real account, dataset, legal right,
aggregation policy, continuity or provider SLA.

## Public references and parser review

Reviewed 2026-10-25, public documentation only; no API/account requests:

- [Historical bars](https://docs.alpaca.markets/us/reference/stockbars): parameters,
  inclusive bounds, variable paging and paired authentication.
- [FAQ](https://docs.alpaca.markets/us/docs/market-data-faq): daily opening instants,
  historical SIP delay, IEX differences and `asof=-`.
- [Plans](https://docs.alpaca.markets/us/docs/about-market-data-api): advertised
  limits are not configured allocations or legal approval.
- [Official Bar model](https://github.com/alpacahq/alpaca-py/blob/master/alpaca/data/models/bars.py):
  timestamp denotes bar opening; no SDK or float-parsing code is copied.

Chrono 0.4.45 is already pinned/resolved, uses MIT OR Apache-2.0 and adds no new
registry versions. Add its direct HTTP-adapter edge for established time parsing;
do not expose Chrono types in public APIs or introduce a vendor SDK.
