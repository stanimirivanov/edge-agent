# Twelve Data daily protocol

## TL;DR

- PR11D implements invented loopback protocol conformance, not live activation.
- Use `DateOnlySchedule` and `TwelveDataDaily`; never fabricate source timestamps.
- Require a pinned native admission, reviewed rights and durable daily credits.
- Run `cargo xtask verify --check twelve-data-conformance`; no provider is contacted.
- Native captures cannot enter the precise daily-v1 worker/storage/publisher.
- PR11F-A adds separate [native quality acceptance](native-daily-acceptance.md);
  PR11F-B still owns native storage, retention and terminal publication.

## Trusted construction and flow

Resolve `SelectionPlan::prepare`, supply every dated calendar including closed
days and explicit reviewed provider-bucket witnesses, then build `DateOnlySchedule`.
It requires capture-only raw provider-day consolidated semantics and at most 366
calendar days/252 open rows. This is a required reviewed capability, not proof that
any real account satisfies it. Feed scope is not listing MIC. Retain/authenticate
the complete reference records and source bytes.

Create the durable native admission and pin `schedule.configuration_digest()` via
`with_configuration`. Configure the exclusive account allocation, known opening
debit, expiry and maximum endpoint price before dispatch. Four starts/minute and
15-second spacing remain stricter local ceilings. Key count never creates quota.
Public documentation describes one credit per symbol; only an authenticated owner
review can install the cost. Do not guess free-tier allowances or opening usage.

`start` checks request/listing/configuration, current authority and embargo before
dispatch. `fetch` reserves/confirms a durable permit around one GET. No pagination,
sleep or hidden retry; a second fetch fails. After lost completion confirmation,
`confirm` retries accounting only, preserving original body receipt and hash.
Dropped/unknown work stays charged and fenced under existing recovery policy.

The fixed endpoint is `https://api.twelvedata.com/time_series`. Encode selected
symbol/MIC, `interval=1day`, `adjust=none`, `order=asc`, `format=JSON`, explicit
start date, final expected open date with explicit local `T23:59:59` query bound,
and `outputsize=366`. The provider's date-only end example excludes that date;
an intra-day upper bound avoids midnight inclusion ambiguity. This is a query
bound, not a source timestamp or a derived provider bucket.
No timezone parameter can promote a native date into a source instant. Send the
required credential only in a sensitive `Authorization: apikey` header. Only the
private unit-test seam accepts loopback HTTP; production has no endpoint override.

Validate symbol/MIC, USD, Common Stock, `1day`, America/New_York, status, native
date syntax, exact OHLCV and the complete ascending expected open-date set. Ignore
additive vendor fields; reject duplicate JSON keys, missing/null critical values,
truncation, extras and reordered rows. Correct metadata alone does not prove
coverage or adjustments. Classify HTTP and HTTP-200 error codes without retaining
message text; errors never refund calls or trigger automatic retries.

Complete-body receipt uses the trusted UTC/monotonic anchor; confirmation preserves
it and derivation uses a later local sample. SHA-256 covers exact response bytes.
Raw retention still needs separate permission.

## Consumer and activation boundary

`DateOnlyDailyArtifact` seals raw native rows with exact decimals and temporal
provenance in an independent schema. `DailyObject::decode` explicitly dispatches
native captures versus precise quality-approved evidence. Unknown, noncanonical,
duplicate-key, digest-mismatched and oversized objects fail closed.

Captures do not imply corporate-action completeness, outlier acceptance, retained
historical knowledge, licensed storage or a terminal ready decision. PR11F-A's
`freeze_native_evidence` rechecks complete quality and explicit owner lineage into
an independent accepted artifact; this is not storage/publication completion. Before PR15,
PR11F-B must add native scoped storage, retention and terminal-message integration,
authenticated runtime catalogs/fencing, non-resetting credit-policy renewal where
needed, and account-specific rights/coverage qualification. The existing worker
remains offline-only and cannot activate the adapter.

## Public protocol references

Reviewed 2026-10-24; public documentation only, no API/account requests:

- [API documentation](https://twelvedata.com/docs): time_series parameters,
  authentication, native daily dates, metadata and error shapes.
- [Timezones](https://support.twelvedata.com/en/articles/5745849-timezones):
  exchange-local equity labels; daily timezone override is inapplicable.
- [Credits](https://support.twelvedata.com/en/articles/5615854-credits): account
  allowances/reset claims require owner review, not hardcoded defaults.
- [Historical bounds](https://support.twelvedata.com/en/articles/5214728-getting-historical-data):
  start/end/outputsize interactions; final-day inclusion still needs live qualification.
- [US coverage](https://support.twelvedata.com/en/articles/9935903-us-equities-market-data):
  distinguish partial real-time volume from delayed historical/EOD claims.
- [Terms](https://twelvedata.com/terms): free-tier pricing does not automatically
  grant automated/non-display, retention or redistribution rights.
