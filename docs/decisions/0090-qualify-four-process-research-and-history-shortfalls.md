# ADR-0090: Qualify four-process Research and history shortfalls

- Status: Accepted
- Date: 2026-10-30
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Qualify real Gateway, MarketData, Research and projector process boundaries.
- Preserve evidence chronology, durable redelivery and exact authoritative bytes.
- Distinguish an owner session shortfall from an unsupported instrument universe.

## Context

Individual adapters and process slices pass, but M04 requires the complete offline
submission-to-query workflow. A combined proof must use actual owner acquisition
and read authorization, not an invented Research loader or calculated output.
Qualification also exposed a distinction hidden by prior tests: a requested lookback
beyond installed history was labeled UnsupportedUniverse despite the existing
InsufficientHistory contract. The maintainer explicitly approved correcting that
narrow classification as part of PR15.

## Decision

Build actual service executables and take paths from Cargo build artifacts. Use
isolated PostgreSQL schemas, an invented MarketData profile, real NATS durables and
actual HTTP admission/query. Acquire historical evidence before the Research cutoff;
then reuse its original receipt during the Research evidence handoff. The test driver
hosts the existing Gateway outbox relay adapter; no autonomous Gateway relay daemon
is implied or added. Calculation, authorized owner reads, completion, projection,
replay and lifecycle operations execute in their actual service processes.

Protect exact test discovery while an owned Python wrapper builds binaries and
forwards the unchanged protected Cargo test argv. Qualify duplicate requests,
observed process death at a SQL write barrier, original-message redelivery after the
real ACK deadline, exact output, read-only replay, disposable rebuild and owner expiry.
Run the gate explicitly through the existing local-platform CI profile.

For new owner requests, an unsupported instrument remains UnsupportedUniverse;
a lookback requiring more sessions than retained returns InsufficientHistory.
Malformed history remains InvalidEvidence. Existing terminal correlation/completion
receipts remain immutable and preserve earlier reasons; no historical rewrite occurs.

## Alternatives considered

In-process adapter doubles do not prove actual boundary composition. Copying an
outbox payload into a fabricated downstream result bypasses the protocol. A new
Gateway relay runtime would add product behavior outside this qualification slice.
Capturing evidence after a historical cutoff and relabeling it as available violates
point-in-time integrity. Shortened test-only ACK semantics would weaken death/retry proof.

## Consequences

This establishes bounded offline process acceptance, not production deployment,
public authentication, provider qualification, autonomous operation or performance.
The real ACK deadline makes one scenario take about a minute. CI protects discovery,
prerequisites and actual execution rather than treating missing dependencies as pass.

## Compatibility and migration

The history-shortfall reason is a behavior change using an already defined v1 enum.
Fresh requests receive the corrected reason; retained retries return their original
receipt. No schema, dependency/version, fixture recipe or original artifact byte
migration is introduced. Rollback removes harness wiring and restores old mapping
for new requests without deleting or reinterpreting retained records.

## Security and operations

Use only loopback infrastructure and uniquely owned test roots/schemas/stream.
Fixture SQL is limited to setup, observation, a write-lock barrier and disposable
projection reset. Production services never read another owner's tables. Process,
HTTP and scenario deadlines are bounded; owned resources are cleaned after success
or scenario failure. No provider/model/cloud/live execution is activated.

## Validation

Protect the build-wrapper command against script/filter/argv drift, both four-process
scenarios and the global ignored-test census. The real owner regression first fails
with the old shortfall reason and then passes with InsufficientHistory, while an
unsupported instrument retains its prior classification. Default checks, real owner
process proofs and full four-service qualification accompany the change.
