# ADR-0033: Freeze deterministic run continuations

- Status: Accepted
- Date: 2026-10-20
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- P05 adds pure simulation-domain values, not a deployable or workflow engine.
- Freeze ready inputs before running; availability precedes source sequence and identity.
- Bind artifacts, code/configuration, algorithms and seed in a SHA-256 manifest.
- Atomically retain domain state, outputs and checkpoints. Continuation alone is not
  distributed fencing, authentication or exactly-once execution.

## Context

P07/P09/P13 need repeatable runs across restart and reordered/same-time evidence.
P04 establishes readiness, not simulation ordering or continuation. Broker arrival,
lock order, wall time and unspecified random generators cannot define these facts.

## Decision

`edgeagent-simulation-domain` depends inward on checked market-data domain values.
`FrozenInput::from_ready` requires P04 readiness and uses its assessment time as
availability. Freeze each revision once from historical readiness; do not relabel
current state with an old timestamp. Later assessments are not exact duplicates.

Freeze at most 4096 descriptors before execution. Sort by availability, feed ID,
epoch, optional sequence (`None` first), observation ID and revision. Reject
conflicting identities/sequences and epoch/revision/source-order regression.
P04 still owns gap, heartbeat and recovery qualification; a sequence counter alone
does not prove continuity. Builders must not silently omit required evidence.

Manifest v1 uses fixed-width big-endian encoding and SHA-256. It pins start time,
implementation/configuration digests, public seed and ordered descriptors.
Configuration must cover the required input set, universe, strategy/parameters,
reference/calendar/rights/policy revisions, numeric rules and environment.
Observation artifacts cover canonical values/provenance; verify their exact bytes
before use. Digests do not certify caller associations, completeness or rights.
Retain canonical manifest bytes and checked descriptors/artifacts to rebuild them;
compare bytes/digest before restoring. No general manifest/payload decoder here.

IDs hash `EA-ID1\0`, manifest digest and big-endian u64 counter with SHA-256.
ChaCha20 supplies random u64 values at stream zero, word offset twice draw count.
Both counters fail before wrap. Seeds/IDs are public reproducibility data, not
security capabilities. Virtual time cannot regress or pass a pending input.

Checkpoint v1 has 108 bytes: `EAC1`, manifest digest (32), cursor (8), UTC seconds
(8 signed), nanoseconds (8), ID counter (8), draw counter (8), SHA-256 (32) of the
preceding 76 bytes. Validate length/version/checksum/manifest/time/cursor before
restore. Restore skips consumed inputs and reconstructs RNG state. Checksum is not
authentication. Older valid checkpoints and concurrent restores can repeat effects;
application-owned atomic commits, fencing and rollback protection remain required.
Discard uncommitted state after crash; retain domain state, outputs and checkpoint
together. Future algorithm/format changes require a new version, not v1 reinterpretation.

## Alternatives considered

Pin Rand ChaCha 0.10.0 and already-resolved RustCrypto SHA2 0.11.1, defaults disabled.
Both maintained upstream projects are MIT OR Apache-2.0. Reviewed
[Rand license/reproducibility](https://docs.rs/rand_chacha/0.10.0/src/rand_chacha/lib.rs.html)
and [SHA2 API/license](https://docs.rs/sha2/0.11.1/sha2/).
No OS entropy or serde features requested; types remain private. Versioned bytes,
reference vectors and restart tests define their replacement boundary.
Reject custom PRNGs, `StdRng`, Rust `Hash`, wall time and random UUIDs.

Putting run lifecycle in market-data domain would mix ownership. Adding workers
or a workflow engine would not solve input determinism or atomic output ownership.

## Consequences

Finite offline foundation only: no durable store, streaming replay, timers, fills,
providers, models or complete M04. P07/P09/P13 add consumers. Reversion needs no
deployed schema/message migration; no existing service imports this crate yet.

## Compatibility and migration

Additive domain crate, no changes to deployed contracts. Preserve v1 bytes and
algorithm vectors; incompatible format changes allocate a new version. Restore
requires the exact original manifest, not migration of its inputs in place.

## Security and operations

Seeds and digests are not credentials. Digests/checksums provide integrity checks,
not authentication, entitlement or proof of complete configuration. No raw payload
or seed appears in diagnostics. Applications must own durable fencing, rollback
protection, atomic state/output/checkpoint commits and authorized artifact retention.

## Validation

Default workspace verification includes restart at every input prefix, reordering,
duplicates/conflicts, feed/epoch binding, clock/resource/counter bounds, malformed
checkpoints, readiness integration and public API compile-fail tests. Independently
computed SHA-256 manifest/ID vectors and the ChaCha20 reference vector protect
retained meaning. See [run guide](../development/deterministic-runs.md).
