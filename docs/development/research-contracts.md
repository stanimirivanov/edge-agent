# Research v1 contracts

## TL;DR

- M04-PR1 defines payloads only; no worker, endpoint or message route is activated.
- Use `ResearchRecord::decode` for bounded canonical bytes and semantic validation.
- Run `cargo xtask verify --check research-contracts --python python3`.

## Boundaries

[ADR-0076](../decisions/0076-version-deterministic-research-contracts.md) records
compatibility and authority. The implementation is in
[`research.rs`](../../crates/contracts/src/research.rs); frozen records and the
local schema are in [`fixtures`](../../crates/contracts/fixtures/research/v1/).

Request scope/ID identifies caller intent. Canonical request content digest detects
conflicting reuse; delivery IDs belong to a future authenticated message adapter.
Artifact digest covers the entire canonical artifact record. It is not a signature.
No URI is resolved. Evidence ownership, rights, formula correctness and lifecycle
state transitions require application validation beyond structural contracts.

## Encoding and compatibility

Payloads have explicit `kind` v1 discriminators and `data`. Required fields cannot
be omitted; unknown fields and variants fail closed. Maximum bytes are 128 KiB,
with at most 64 candidates/evidence references. Evidence is a bounded census of
retained multi-observation artifacts, not one entry per price bar. Scores are
integer parts per million; price units and adjustment rules belong to the pinned
configuration and referenced evidence. Policy selection is not inferred here.

Canonical JSON sorts object keys recursively, preserves array order, and has one
final newline. This is the repository encoding, not arbitrary RFC 8785.
Duplicate keys, extra whitespace and alternate ordering are rejected at decode.
Raw DTO deserialization is explicitly unvalidated. Use `canonical_bytes` to validate
constructed DTOs. Scenario/risk illustrations use `not_qualified` rather than null,
zero or invented numeric values. A future qualified contract requires review.

## Verification

The `research-contracts` default T2 gate runs with schema generation and
`serde_json/preserve_order`. Tests retain exact v1 examples for all record kinds,
reject malformed versions, identities, chronology, ranking and oversized input,
and compare the generated schema with its checked-in inventory. Schemas are
structural; semantic validation remains mandatory. Update a schema only through
`research::schemas::generate`, review the diff and preserve compatibility fixtures.
No schema URL is fetched and no network or service is required by these tests.
