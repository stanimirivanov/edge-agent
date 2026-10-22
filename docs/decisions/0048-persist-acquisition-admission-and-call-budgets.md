# ADR-0048: Persist acquisition admission and call budgets

- Status: Accepted
- Date: 2026-10-22
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Persist immutable admission and atomic account/command accounting in PostgreSQL
before external calls. Preserve uncertain charges and fences across restart;
never release them on lease expiry.

## Context

HTTP and storage adapters cannot enforce shared budgets across replicas. Lost
confirmations must not reset deadlines, charge attempts twice, refund twice or
permit overlapping provider calls.

## Decision

Use an application-owned `AcquisitionLedger` with SQLx checked-query adapter.
Canonical versioned bytes bind the full request, quality/listing and authority
pins. Identical admission recovers; changed content conflicts. Preserve original
deadline/retention. Lock account before command, sample database time after locks
and commit before returning permits. No external I/O occurs inside transactions.

Explicit idempotent attempt ordinals preserve counters. Reserve response maximums
before I/O; retain immutable completion receipts to reject conflicting outcomes
and protect newer fences. Share spacing, rolling starts and provider delays.
Limits may tighten, never silently relax. Cancellation/expiry deny new starts
without releasing unresolved work. Only trusted owner-confirmed local quiescence
allows unknown-outcome reconciliation, retaining the entire charge.

## Alternatives considered

- Process-local counters: disappear on restart and multiply across replicas.
- Expiring permits: allow overlapping I/O when paused workers resume.
- Refund on timeout: mistakes uncertainty for absence of external effects.
- Build the provider worker now: mixes accounting with runtime/publication slices.

## Consequences

Accounts can remain blocked until an owner establishes quiescence. Serialization
intentionally bounds daily-profile throughput. Recover uncertain commits with the
same identities, not replacements. No automatic retries or remote exactly-once
guarantee are introduced.

## Compatibility and migration

Add private account/admission tables and an adapter crate. Existing CloudEvents
and evidence formats remain unchanged. Apply the additive migration before use;
reverting code leaves inert retained tables. PR10 owns terminal publication.

## Security and operations

Records/permits identify work, not current data rights. Composition authenticates
catalogs and rechecks rights at dispatch. No provider/model/credential activation.
Pool bounds and schema ownership remain composition obligations. The later
runtime owns worker fencing/concurrency. PR12 owns physical deletion.

## Validation

Six default model proofs and three database proofs cover immutable admission,
contention, reconstruction, unknown charges, cancellation and stale completions.
Exact harness censuses reject missing/renamed/extra tests. SQLx offline metadata
is generated against PostgreSQL; freshness assembly includes all existing schemas.
