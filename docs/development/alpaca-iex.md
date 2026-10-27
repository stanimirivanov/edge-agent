# Alpaca IEX offline streaming protocol

## TL;DR

- PR19 adds a library for IEX trades and minute bars, not a vendor runtime mode.
- Exact frozen symbol subscriptions follow welcome and authentication acknowledgement.
- PR18 confirms every frame before bounded complete-message decoding and selection.
- IEX is venue coverage, never historical SIP or consolidated evidence.
- Corrections/cancels stop intake; PR20 owns semantic acceptance, reconciliation and readiness.

## Scope and configuration

`AlpacaIex` in `market-data-http` fixes the endpoint to
`wss://stream.data.alpaca.markets/v2/iex`. There is no endpoint override, automatic
feed fallback, wildcard, environment lookup, credential discovery, retry or reconnect.
Construction performs no provider call and installation grants no rights or activation.
Credentials are supplied explicitly and sent only in an authentication message over
verified TLS; debug/error formatting excludes credentials, symbols and market values.

Supply every `WssConfig` limit, an entire protocol handshake budget of 1..10000 ms,
and 1..1024 records per JSON message. Complete messages are capped by the configured
assembly budget, never above 1 MiB. Incoming/outgoing frame budgets still come from
PR18 and the sealed admission. Account capacity includes unresolved starts: neither
EOF, Drop nor successful local close authorizes a release or refund.

## Contracts and normalization

The session must declare `MarketScope::Venue(Venue::Xiex)`, nanosecond source time
and snapshot delivery. Alpaca's JSON feed has no qualified contiguous sequence;
trade IDs, counts, frame ordinals and timestamps are not sequence substitutes.
The complete frozen selection must match the session witness before a start is spent.
Keep the listing venue from its exact reference mapping separate from IEX price
discovery scope; an instrument listed on Nasdaq can have IEX observations.

Each existing session admits one evidence contract: trades OR intraday minute bars.
Both are supported; this PR does not invent simultaneous multi-contract admission
or evade account limits by opening extra connections. A future combined feed requires
an independently reviewed multi-contract accounting/authority design.

Trades retain exact price/positive share size, source trade ID, exchange, tape,
original condition codes and nanosecond event time. Minute bars retain exact OHLCV,
trade count and VWAP and require a minute-aligned 60-second half-open bucket.
Intraday coverage includes extended hours; it does not assert a regular session.
`IexObservation` is an adapter result, not `CanonicalObservation`: PR20 must resolve
authenticated semantics, condition eligibility, calendar/session, deterministic
logical identity/revision and any reconciliation before capture/publication.
Receipt comes from the final charged fragment; no provider timestamp is fabricated.

## Protocol and failures

Welcome precedes JSON authentication; its acknowledgement precedes one exact
subscription. Verify the entire subscription acknowledgement including automatic
trade correction/cancel channels. Reject missing/extra/duplicate symbols and
unexpected nonempty channels. Control messages cannot be hidden by symbol selection.

Confirm frame accounting before assembling fragmented text, with ping/pong processing
through the same charged WSS path. Validate complete UTF-8 and bounded duplicate-free
JSON, then inspect type/symbol before selected payload conversion. Known unselected
data skips value decoding; unknown mappings fail closed. Validate the complete batch
before returning records, so a later error never releases an earlier partial result.
Local item offsets preserve positions before filtering; frame spans are local capture
coordinates, never continuity evidence. No replay/deduplication is inferred here.

Any provider error, unsupported message, correction/cancel/updated bar, malformed
payload or acknowledgement stops the owner. A reconciliation result means prior
capture must not be treated as final evidence; it does not undo already published
data. PR20 must preserve that invalidity across restart. Failed/abandoned operations
cannot reuse the local socket. Current authority, cancellation, idle and absolute
session lifetime remain enforced by PR18; controls cannot renew the protocol deadline.

## Verification and remaining gates

Run `cargo xtask verify --check alpaca-iex-conformance`, then `cargo xtask verify`.
Synthetic fixtures and ephemeral local TLS peers require no credentials or Docker.
The protected proof census also runs under JSON-order/compression feature unification.
No Alpaca endpoint, including its public test stream, is used by these tests.

PR20 remains responsible for fair supervision, authenticated fresh catalogs, semantic
acceptance, durable correction invalidation/recovery, capture publication and readiness.
Live use separately requires rights, credential/account review, combined HTTP/stream
entitlement accounting and operational qualification. Daily live qualification remains
not run. Rollback stops using this additive API without deleting counters or reservations.

Protocol references reviewed on 2026-10-27:
[WebSocket protocol](https://docs.alpaca.markets/us/docs/streaming-market-data) and
[stock schema](https://docs.alpaca.markets/us/docs/real-time-stock-pricing-data).
Adapt protocol ideas only; no external implementation or licensed data was copied.
No new dependency or licensing exception is introduced.
