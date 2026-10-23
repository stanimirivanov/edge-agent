# Versioned instrument selection and early routing

## TL;DR

- Explicit immutable selection/directory snapshots narrow requests before provider calls.
- Process validated framing, controls and qualified sequence metadata before filtering.
- Unknown keys fail closed; empty selection means none, never all.
- Bind actual configuration and revisions into P05 replay; hashes are not authority.
- This is a synthetic foundation, not a live subscription or native feed qualification.

## Compile the pinned plan

[ADR-0052](../decisions/0052-freeze-instrument-selection-and-route-headers-before-payloads.md)
defines the application-owned boundary. Store `InstrumentSelection` and
`RoutingDirectory` in the existing `ReferenceHistory`/`ReferenceVersion` types.
The trusted owner authenticates source bytes, revisions and locate associations;
constructors or caller-provided checksums cannot establish legal authority.

Selection contains at most 32 distinct instruments, matching P04's plan ceiling.
An empty set stops all data. A directory contains at most 4096 explicit symbol or
nonzero locate keys in one source/feed/epoch namespace. It is not a complete-market
security master. Reuse canonical `InstrumentMapping` witnesses; aliases remain
case-sensitive, and symbol keys must agree exactly with their mapping. Reject
duplicate keys and conflicting instrument/listing mappings, even when unselected.

Resolve exact selection/directory IDs with `SelectionContext`: source/feed/epoch,
effective UTC instant, inclusive knowledge cutoff and explicit mapping session.
Choose latest known revisions before checking scope/effective dates. Corrections
cannot resurrect old matching records. Embedded mappings cannot be known after
their directory; every selected instrument must be mapped. Directory replacement
is atomic. Missing coverage is an error, not a fallback or proof of completeness.

## Narrow outbound work

`SelectionPlan::prepare` validates the immutable request and seals `SelectedRequest`
only for matching source, admission/knowledge context and a selected instrument.
The alias must cover the complete requested date range; segment real alias changes
explicitly rather than querying one guessed ticker. The prepared route exposes the
exact alias/listing witness for later provider URL/subscription encoding.

Wrap an existing `DailyProvider` with `SelectedDailyProvider` to reject excluded or
cancelled work before invoking its implementation. This decorator does not replace
rights checks, durable call permits, whole-series quality or response validation.
The future worker must compose all of these boundaries, not call an unguarded
provider or treat selection as entitlement. Narrow requests at a provider when its
qualified API supports it; no real subscriptions or endpoints are sent here.

`SyntheticDailySource::new` now requires `SelectedRequest`, not a raw request.
It automatically includes actual selection/directory content and version witnesses
in P05 configuration binding, alongside the existing caller configuration digest.
Old or differently configured checkpoints reject. No selection database, durable
configuration pinning or user-facing watchlist API exists in this slice; PR13 owns
authenticated runtime configuration and persistence. Admission-v1 stays unchanged.

## Route before decoding

`EarlyRouter` receives validated `FrameMetadata`, never raw provider bytes.
Adapters must validate complete framing/size and classify controls first. Each
header checks source/feed/epoch. An explicitly qualified `ContinuityMode::Contiguous`
checks every sequence, including unselected data and heartbeat controls; gaps,
duplicates, missing sequence and overflow invalidate. `Snapshots` makes no
gap-free claim even if a frame happens to contain a sequence number.

Known selected keys return their exact mapping for full decoding. Known unselected
keys skip payload decoding. Unknown keys invalidate rather than silently dropping.
Heartbeats remain explicit outputs that the owner must forward to P04. Reset,
directory-change and auth-failure controls invalidate even with an empty selection.
No later data, heartbeat or directory message heals the cursor. Reconstruct only
from an externally qualified new snapshot/baseline, respecting P04 recovery rules.
`is_valid` is a routing-latch status, not heartbeat freshness or data readiness.

Do not feed a filtered global counter into a per-instrument P04 contiguous tracker
as though its selected observations had dense sequences. Skipped rows consume
header progress but do not synthesize canonical observations. Qualified streaming
must bridge feed-level progress, selected evidence and atomic checkpoints in its
later protocol/recovery slices; this fixture does not establish that bridge.

The replay adapter's bounded EAS1 framing is invented fixture encoding only.
`route_synthetic_frame` checks the complete frame before routing, invokes the
supplied pure payload decoder only for selected data, and invalidates on decoding
failure. Control frames have no body. It performs no I/O, implicit recovery or
vendor mapping. Real adapters must implement their own qualified framing/control
semantics; do not claim ITCH/WSS support from this fixture.

Local filtering saves CPU/allocations, not received bytes, bandwidth or billed
calls. Licensed raw retention and privacy rules still apply to incoming buffers.
Never log aliases, payloads, configuration bytes or catalog diagnostics. A frozen
plan is not an indefinitely live subscription: PR13/PR16 must enforce current
authority, bounded lifetimes, ownership and qualified streaming recovery.

## Verification

```text
cargo xtask verify --check instrument-selection-routing
cargo xtask verify --check offline-source-isolation
cargo xtask verify
```

Six protected routing tests and an independent census mutation guard cover history,
request rejection before calls, early filtering, controls, namespaces, sequence,
framing, exact fixture encoding, canonical configuration and redacted diagnostics.
The existing six source proofs now exercise checked selection and reject a replay
checkpoint after a selection revision changes. They use invented data and require
no provider, credentials, database, network service or container.

Linux CI separately executes the offline source suite under network denial; the
portable header tests do not claim OS isolation, native feed readiness, durable
ownership, throughput benchmarks or live qualification. No new Cargo dependency
or protocol-specific library is introduced.
