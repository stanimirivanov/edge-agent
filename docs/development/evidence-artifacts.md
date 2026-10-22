# Versioned evidence artifacts

## TL;DR

- `contracts::evidence` owns immutable daily-v1 wire DTOs, local schemas and routing.
- `market-data-application::freeze_evidence` rechecks acquisition authority and daily quality.
- Canonical bytes bind exact values, scope, policy and retained reference witnesses.
- Objects remain outside messages; hashes never grant permission or establish source authenticity.
- This contract slice activates no provider, storage, worker, read or deletion runtime.

## Encoding and validation

The daily-v1 format uses compact recursively key-sorted JSON with exactly one
final LF. SHA-256 includes that LF. This is a specific encoding, not a general
RFC 8785 implementation. Reordered reference witnesses normalize before sealing;
other alternate bytes, duplicate/unknown keys and unsupported versions fail
`DailyEvidenceArtifact::decode`. Read adapters must bound streamed downloads
before allocation, then call this bounded decoder with the expected hash.
The ceiling is 4 MiB, including the LF; message envelopes retain their existing
256 KiB ceiling. No large-object exemption applies to the bus.

Prices are positive USD decimal coefficient strings with scale 0..8; volume
uses nonnegative share coefficients with scale 0..6. Normalized coefficients
contain at most 28 digits. IDs are nonzero lower-case 128-bit hexadecimal text;
revisions and policy-age milliseconds use exact positive integer text. Optional
source sequences also use integer text, preserving zero and the full u64 range. UTC
seconds/nanoseconds retain provider precision; header/reference times and bucket
boundaries remain millisecond-aligned. Inclusive knowledge cutoff, capture
time, half-open date range, optional provider times, and exclusive deletion
deadline are distinct. Canonical retention cannot exceed the admitted 24-hour
ceiling. Explicit null is required for absent nullable fields.

Local Draft 2020-12 schemas check wire shape without remote URI resolution.
Rust validation additionally checks exact decimal relationships, valid Gregorian
dates, chronology, bounded coverage and revision lineage. Schema validity alone
does not establish quality, rights or truth. Never log raw serde errors, prices,
condition codes or provider bodies; checked codecs expose payload-free errors.

## Producer boundary and retained provenance

Freezing rechecks the admitted request against current rights/provider catalogs,
then runs the existing whole-series quality policy. It seals the selected
observation and predecessor revisions, transformation/semantics revisions,
quality thresholds and every requested calendar day, including closed days.
Each reference witness pins role, logical version, source authority, knowledge
time and source-byte checksum. The owner must retain the complete referenced
mapping/calendar/action/coverage/rights/review records for replay; witnesses are
not standalone copies of those records or object-store locators.

The v1 producer freezes one qualified primary source. A comparison threshold
does not imply that comparative-source evidence was collected or licensed.
Adding independently licensed comparative provenance requires a new explicit
contract, not silently claiming that this artifact contains it.

Corrections create a new artifact and link `replaces` to the prior digest.
The producer requires a new request identity, the same scope/range/semantics
and logical rights ID, and nonregressing knowledge/capture time. It does not
authorize reading predecessors or serialize concurrent corrections: the durable
owner must authenticate prior reads and fence publication against its current
head. Never overwrite published bytes or deduplicate across licensed scopes.
The owner must also enforce observation-ID/revision immutability across captures;
checking one batch or its hash cannot authenticate a globally consistent history.

## Message and ownership contracts

`AcquireEvidence` contains only the immutable admission-record ID and SHA-256.
The owner must load and authenticate that record and verify its digest before
freezing; this PR does not define a canonical admission-record encoding.
The mapper checks identity and preserves the authenticated digest, not its
correspondence to caller-supplied request fields.

`EvidenceDecision` is one terminal ready/rejected outcome. Ready binds the exact
digest/length, storage scope, audience, rights version, deletion deadline and
optional predecessor; rejection carries a stable typed reason, not free text.
Both routes require `acquisition/{request-id}` partition identity. The event
must originate from MarketData. There are no prices, URLs, credentials, actor
assertions or signed read capabilities on the bus. Storage adapters must resolve
objects under storage + audience + rights authority; content hashes alone are
not global public keys. Consumers must recheck authority and expiry on reads.

Registry definitions are additive and are not yet activated in deployment
composition. Later M03 persistence/publication work must commit authoritative
decisions through the transactional outbox and preserve inbox deduplication.
Storage publication, admission-record durability, correction-head fencing,
retrieval, revocation and deletion proofs remain later M03 work.

## Verification and evolution

Run `cargo xtask verify` for default acceptance. Focused gates are:

```text
cargo xtask verify --check evidence-contracts
cargo xtask verify --check evidence-freezing
```

Both gates discover an exact named-test census before execution. Tests cover
canonical bytes, schema compatibility, malformed/tampered objects, payload-safe
diagnostics, exact routing/ownership, producer quality and scoped corrections.
The wire gate enables `serde_json/preserve_order` deliberately.

The explicit maintainer-only generation command is:

```text
cargo run --locked -p edgeagent-event-spine-conformance --example evidence_schemas -- --write
```

Without `--write` it verifies exact artifact bytes and directory inventory.
Generation never updates `fixtures/evidence-baselines.txt`; that independent
hash floor freezes all three schemas and four synthetic goldens. Published-v1
changes must retain these artifacts and introduce a new version plus explicit
compatibility handling rather than refreshing the floor to hide drift. This
PR introduces the first v1 baseline; it does not migrate persisted data.

Runtime contracts use existing pinned `chrono` and `sha2` (MIT/Apache-2.0).
Existing pinned `schemars` (MIT) is optional under `schema`, enabled only by
tooling. No provider, cloud, object-store or network client dependency is added.
