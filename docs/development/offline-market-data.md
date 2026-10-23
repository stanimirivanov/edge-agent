# Offline market-data source and replay

## TL;DR

- `market-data-replay` supplies invented daily observations, not an activated worker.
- Missing mock configuration selects offline; false/invalid flags do not enable live access.
- Exact invented bytes and P04 readiness bind existing P05 manifests and checkpoints.
- Default proofs use no provider service; Linux CI separately disables network routing.

## Construction and boundaries

[ADR-0050](../decisions/0050-isolate-invented-daily-sources-and-reuse-frozen-replay.md)
defines `OfflineMode` and `SyntheticDailySource`. Supply the raw optional mock flag
to `OfflineMode::from_mock_flag`; only absence, `1` and `true` succeed. The library
does not inspect environment variables, load secrets, create clients or read clocks.
The bootstrap MarketData service is unchanged; this is not a new CLI/env setting
on that service. PR13 owns runtime composition and authenticated configuration.

Build an explicit synthetic `AcquisitionRequest`, invented canonical observations,
P04 continuity trackers and a checked readiness plan. Serialize each record with
`synthetic_daily_bytes`, retain those bytes in plan order and supply them to the
constructor with implementation/configuration digests and a reproducible seed.
The configuration digest identifies synthetic rights, calendar/reference, quality
and selection revisions; the adapter additionally binds exact request scope,
dates, deadlines and resource ceilings. A digest proves bytes, not permissions.

The private fixture encoding uses a version tag and explicit fixed-width fields.
No floating point, `Debug` serialization, system clock, random seed generation or
provider body is involved. Constructor checks exact bytes, cardinality, total
4 MiB bound and request provenance before retaining observations. Only venue-scoped,
millisecond, provider-day capture snapshots are supported; no historical licensed
replay, index/trade/quote, cancellation or sequenced-feed encoding exists here.
P04 readiness limits one plan to 32 distinct feed/instrument requirements. This
finite adapter does not claim a 252-session daily acquisition implementation.

Invoke `DailyProvider::fetch` with matching current checked authority. Fetch is
repeatable and never consumes a consumer cursor. Run `validate_batch`, daily
whole-series quality and normal publication authority checks afterward. Synthetic
provenance belongs in the supplied source/semantic catalogs; this adapter cannot
authenticate a claim that arbitrary caller-supplied observations are invented.

## Continuation

`replay()` creates an independent single-owner P05 run. `next` exposes one original
observation, a deterministic effect ID and a simulation RNG draw. The run clock
uses readiness availability, never earlier provider/event time. This slice binds
one capture request at its admission assessment instant; it does not simulate an
unbounded live clock or refresh receipts during replay. Cancellation leaves the
checkpoint unchanged. `restore` checks P05 version, checksum, manifest and state.

Persist accepted outputs and checkpoint atomically in the owning application.
Failed/uncommitted work may repeat. Valid older checkpoints and concurrent owners
need external fencing; this library grants neither exactly-once effects nor durable
ownership. Lost terminal publication is recovered by PR10's receipt-first path.

## Verification

```text
cargo xtask verify --check offline-source-isolation
cargo xtask verify
```

Six protected tests cover default/invalid mode, nonempty repeatable canonical
batches, every-prefix restoration, corrupt/mismatched/oversized fixtures,
cancellation, foreign scope and a child with fake provider credentials. The child
does not modify the parent's environment. Tests use invented prices and dates.

On Linux with cached dependencies, noninteractive sudo and unshare:

```text
cargo xtask verify --check offline-source-network-denial
```

The script builds the locked test executable with Cargo offline, then runs the
entire suite in a fresh network namespace. It installs nothing, fetches nothing
and fails on missing isolation rather than falling back. Default portable tests
are not a substitute for this OS-level proof. Runtime dependency/download traffic
is distinct from live-provider traffic; no proof contacts a provider or model.
