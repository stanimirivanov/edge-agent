# ADR-0050: Isolate invented daily sources and reuse frozen replay

- Status: Accepted
- Date: 2026-10-23
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Implement a network-free invented daily adapter behind `DailyProvider`. Default
mode selection is offline; false/malformed mock flags grant no live capability.
Reuse P04 readiness and P05 manifests/continuations, not another replay engine.
No MarketData worker or live provider is activated.

## Context

Provider development and agent tests need meaningful evidence without live quotas.
An empty mock, ambient credentials or an automatic live fallback cannot prove this.
Existing daily acquisition, authority, quality, storage and atomic publication
contracts remain authoritative. Provider selection remains deferred by ADR-0032.

## Decision

Add the adapter-only `market-data-replay` crate with exactly the application,
market-data domain and simulation-domain dependencies. Accept checked invented
observations and exact bounded fixture bytes; expose no client/credential factory.
Caller-supplied `OfflineMode` parsing defaults offline and rejects live requests.

`SyntheticDailySource` binds one exact capture request and P04 readiness plan to
canonical invented daily records. The fixed-width tagged encoding includes source,
semantics, instrument, transformations, identity, revision, optional sequence,
conditions, dates, all timestamps, bucket bounds and exact decimal parts. Scope,
deadlines, budgets and caller configuration bind the P05 manifest. Reject changed
bytes and unsupported contracts before replay. This is a fixture format, not a
public market-data event or a claim that arbitrary supplied data is invented.

Fetch returns a repeatable nonempty canonical batch without advancing a replay
cursor. A separate single-owner cursor uses existing P05 clock, IDs, RNG and
checkpoint validation. Restore skips consumed inputs; cancellation does not
advance state. No effect/commit/distributed ownership guarantee follows.

Protect six named offline proofs and their independent census. Linux CI executes
the complete compiled suite inside a fresh network namespace with no external
routes; build outside it with locked cached dependencies. Failure to provision
isolation fails that check, never becomes a passing fallback.

## Alternatives considered

- Import an upstream daemon: rejects the existing service ownership, exact
  contracts and dry-run-only capability boundary; adds unnecessary dependencies.
- Put another replay engine in acquisition: duplicates P05 algorithms/checkpoints.
- A boolean that constructs live clients unless true: unsafe ambient activation.
- Claim default tests prove OS isolation: dependency restrictions and offline
  behavior are distinct from the separately executed Linux namespace proof.

## Consequences

### Positive

Meaningful repeatable synthetic source output and continuation need no provider,
network service, credentials, database, container or host clock.

### Negative

Fixtures require supplied checked readiness and exact canonical bytes. One P04
plan has at most 32 distinct feed/instrument requirements; this source is not a
general full-history loader. Snapshots may not claim contiguous sequence coverage.

### Neutral or follow-up

The approved [M03 acquisition plan](../development/plans/m03-acquisition.md)
retains metered dispatch, selection, connectors, retention, runtime and recovery
as separate slices. Licensed replay, live configuration and streaming are absent.

## Compatibility and migration

Additive Rust adapter API and one root lockfile package. No public wire change,
database migration or service activation. Existing acquisition configuration is
not replaced; PR13 will integrate explicit mode selection at the composition root.
Remove the unused adapter to roll back; no retained authoritative state changes.

## Security and operations

Invented fixtures and synthetic source/reference identifiers only. No recording
of vendor prices, no secrets, no fallback, DNS or provider probes. A caller-supplied
checksum/readiness result is not authentication, license permission or complete
daily quality. Revalidate current authority and whole-series quality after fetch.
Persist outputs/checkpoints atomically with owner fencing outside this adapter.
The Linux proof uses noninteractive sudo only to isolate a synthetic test child;
no live secrets are supplied. Windows retains the protected portable tests.

## Validation

Run `cargo xtask verify`, including `offline-source-isolation`, both architecture
policies and the independent census mutation test. Linux CI additionally runs
`cargo xtask verify --check offline-source-network-denial`; unavailable Linux
isolation is not-run/nonzero, not a success. Dependency changes require the
supply-chain profile. No synthetic proof qualifies a live provider or data rights.
