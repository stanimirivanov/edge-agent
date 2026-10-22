# ADR-0045: Freeze versioned evidence artifacts and terminal contracts

- Status: Accepted
- Date: 2026-10-22
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Freeze bounded, exact daily evidence independently of message transport.
- Reuse checked acquisition authority and whole-series quality at the producer.
- Publish scoped object identities, never bulk prices or read capabilities.
- Pin local schemas and synthetic bytes with independent compatibility baselines.

## Context

Daily quality and provider conformance exist, but neither defines immutable
evidence bytes, correction linkage or transport-owned acquisition outcomes.
Point-in-time consumers need stable scope, exact values and retained lineage
without importing provider/domain implementations into wire contracts.

## Decision

Add daily-v1 DTOs and a bounded canonical SHA-256 codec to contracts. Extend the
application's inward dependency set with contracts only for explicit mapping;
the domain remains transport-free. Freeze exact policy, selected observation
revisions and reference witnesses after rechecking authority and quality.
Preserve closed-day calendar coverage in the checked domain selection.

Define an identity-only acquire command and one authoritative MarketData
ready/rejected decision. Ready references immutable scoped objects; messages
contain no bulk rows, URLs, credentials or invented source assertions.
Corrections append a new scoped digest and predecessor link. The durable owner
will fence publication and authorize predecessor reads in later M03 slices.

Use existing pinned chrono/sha2 and optional tooling-only schemars. Maintain
strict local schemas, synthetic golden artifacts and independent hash floors.
See [the evidence guide](../development/evidence-artifacts.md) for exact units,
encoding, scope, limitations and verification commands.

## Alternatives considered

- Inline evidence in events: rejected; conflates object and envelope limits and
  distributes licensed rows unnecessarily.
- Serialize domain types directly: rejected; leaks implementation invariants
  and prevents independent wire evolution.
- Hash arbitrary provider JSON: rejected; unstable ordering/precision and no
  canonical authority, policy or scope binding.
- Add an object-store client now: rejected; persistence and authorized reads
  belong to later independently testable M03 behavior.

## Consequences

Reproducible bytes bind complete wire scope, policy and lineage. Full reference
records still require owner retention; source checksums do not authenticate
sources. The initial producer supports one qualified primary source, not an
unrecorded comparative-source quality claim. Objects are at most 4 MiB; bus
envelope limits remain unchanged. No provider is activated.

## Compatibility and migration

The two registry entries are additive and initially unprovisioned. Existing
message definitions and persisted records do not change. Published v1 schemas
and fixture hashes cannot be refreshed to conceal incompatibility. Breaking
evolution requires retained old versions and explicit new routing/decoding.
Canonical encoding includes one final LF and remains JSON-feature independent.

## Security and operations

Treat all input as untrusted. Bounds precede parsing, diagnostics redact payloads,
and producer checks fail closed. Hashes establish byte identity, not licenses,
source trust or permission to read. Current authority and expiry must gate
future retrieval; storage keys must bind rights/audience/storage, not hash alone.
Admission-record authentication and digest verification remain owner duties.
Publication, retention/deletion enforcement and correction-head fencing are not
implemented by these pure contracts.

## Validation

Default verification includes protected evidence wire and producer tests with
exact discovery censuses, compatibility hashes, routing checks and negative
cases. Canonical tests enable preserved JSON object order. Domain/application
dependency rules reject transport leakage. Supply-chain verification covers
root and independent proof lockfiles; runtime/SBOM prerequisites remain explicit.
