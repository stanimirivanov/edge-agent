# ADR-0052: Freeze instrument selection and route headers before payloads

- Status: Accepted
- Date: 2026-10-23
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Freeze explicit selection/directory revisions before provider invocation. Validate
framing, control and qualified sequence metadata before filtering data headers.
Unknown mappings never become accept-all. No provider or worker is activated.

## Context

Mutable watchlists/global locate maps can change acquisitions, reuse another feed's
keys and hide controls or gaps in excluded traffic. Existing reference history,
canonical mappings, P04 continuity and P05 replay already own related invariants.
No vendor protocol has been selected.

## Decision

Keep application-owned `InstrumentSelection`, `RoutingDirectory` and resolved
`SelectionPlan`. Reuse reference histories/versions, canonical `InstrumentMapping`
and source/feed/epoch identities. Select latest known revisions before checking
scope/effectivity. Directory snapshots replace atomically; mapping receipts cannot
follow their directory receipt. Every selected instrument needs an unambiguous
mapping. Bound sets to 32 instruments (P04's ceiling), directories to 4096 keys.
Empty selection means none. Neither resource ceiling defines a licensed universe.

`prepare` seals `SelectedRequest` for exact source, admission/knowledge context and
whole-range alias coverage. `SelectedDailyProvider` rejects excluded requests before
provider invocation. Entitlement, durable metering, conformance and quality remain
separate mandatory boundaries. The offline source constructor requires the sealed
request and binds actual selection/directory values and witnesses into P05's
configuration digest; caller-provided checksums cannot hide changed local content.

`EarlyRouter` checks every header's namespace and qualified contiguous sequence
before filtering, reusing `ContinuityMode`. Snapshots make no gap-free claim.
Known unselected data skips decoding. Heartbeats remain explicit outputs; auth
failure, reset, directory change, unknown key, gap or malformed framing latches
invalidity. No data heals it. This cursor is not another P04 recovery engine or
readiness grant. A bounded invented EAS1 frame adapter proves framing-before-routing
and lazy decoding without I/O; it is not a vendor protocol or public event contract.

## Alternatives considered

- Import a daemon/bus: duplicates service ownership; use existing domain abstractions.
- Filter decoded DTOs: does not avoid full deserialization work.
- Filter before control/sequence processing: hides auth failures, resets and gaps.
- Missing directory means all: expands capability precisely when mapping fails.
- Mutable global keys: breaks epoch isolation and frozen replay.
- Native ITCH/WSS now: selected protocols and streaming authority are prerequisites.

## Consequences

Provider-supported request narrowing can avoid calls; local filtering saves decode
work, not already-received bytes or billed calls. Ambiguity sacrifices availability
rather than guessing. Whole-range alias changes require explicit segmentation.
Only externally qualified reconstruction can resume an invalidated router.

## Compatibility and migration

Application APIs are additive. BREAKING Rust offline constructor input: replace
raw requests with `SelectionPlan::prepare`'s `SelectedRequest`. Automatic selection
binding changes synthetic manifests; old checkpoints reject rather than resume
under changed configuration. No daily-v1 wire, SQL, dependency, migration, deployment
or live behavior changes. Revert needs no persisted production-state migration.

## Security and operations

Use invented fixtures. Redact aliases, payloads and configuration from diagnostics.
Catalogs and locate associations require owner-authenticated provenance; hashes
grant no permission. A frozen plan is not a live subscription or current entitlement.
PR13 owns authenticated configuration, lifetimes, durable pinning and composition;
PR16+ owns qualified streaming authority/recovery. Admission-v1 does not persist
this configuration. No model/broker calls, fallback or quota changes are introduced.

## Validation

`cargo xtask verify --check instrument-selection-routing` protects six named tests
and an independent removal/rename/ignore census. `offline-source-isolation` now
exercises selected source output and checkpoint rejection after changed selection.
`cargo xtask verify` includes both gates and architecture/documentation policy.
Linux CI retains separate source-suite network denial. Portable header tests are
not OS isolation, native feed readiness, durable ownership or live qualification.
