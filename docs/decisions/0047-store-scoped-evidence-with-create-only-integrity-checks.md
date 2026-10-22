# ADR-0047: Store scoped evidence with create-only integrity checks

- Status: Accepted
- Date: 2026-10-22
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Use an application-owned evidence-storage port and an `object_store` adapter.
Create exact canonical bytes atomically under licensed scoped keys; verify every
read and write confirmation. Require current owner-authenticated access checks.
No cloud backend, public read route or deletion worker is activated.

## Context

Daily-v1 evidence already binds exact bytes, rights, scope and retention.
Messages carry object references rather than payloads. An interrupted write can
commit even when its caller receives no confirmation. Hash equality alone grants
neither a license nor permission to reuse an object across audiences.

## Decision

The application owns `EvidenceStorage`, `EvidenceAccess` and typed failure kinds.
The adapter owns SDK calls, an explicit positive timeout capped at 15 seconds,
bounded collection and conditional creation. Keys contain serialization version,
storage scope, audience, rights ID/revision and SHA-256. Checked hex/integer
segments prevent arbitrary path or URL input. Dedicated backend namespaces must
not be shared with untrusted writers.

Use `PutMode::Create`, never overwrite or an existence-check/PUT race. Successful
and already-existing writes both require read-back of the full canonical bytes.
Reads verify metadata length/full range, enforce the length ceiling before
accumulation, decode SHA-256/canonical JSON, and match the entire descriptor.
ETags are not content hashes. Corrections append objects, preserving predecessors.

`EvidenceAccess` is a trusted owner extension point, not a remote DTO or bearer
capability. The owner resolves authenticated actors and retained admission records
and rechecks current rights, including revocations. There is no permissive default.
The adapter independently checks the exclusive deadline using the injected clock
before I/O, during streaming and before returning bytes. It checks current authority
again after I/O. Later runtime composition must supply the real policy.

Pin `object_store` 0.14.2 (MIT/Apache-2.0) with only filesystem support enabled.
Its [conditional-write API](https://docs.rs/object_store/0.14.2/object_store/enum.PutMode.html)
provides the portable primitive; no hand-written S3 protocol or filesystem
atomic-write implementation is needed. Memory is synthetic; local files prove
adapter reconstruction, not machine/power-loss durability or cloud semantics.

## Alternatives considered

- Direct filesystem writes: duplicate conditional-create mechanics and impede portability.
- Unconditional SDK PUT: overwrites corruption and races with concurrent writers.
- Global hash-only keys: disregard licensed storage/audience/rights boundaries.
- Immediate cloud activation: premature without reviewed IAM, encryption, retention
  and account-specific conditional-write qualification.

## Consequences

Read-back costs one bounded download after each write, including duplicates.
Cancellation, timeout and backend errors do not imply rollback. Retry the same
artifact/key to recover; do not delete or mint a new identity as compensation.
Unsupported conditional creation fails closed. The adapter does not retry itself.
SDK source errors remain explicitly inspectable but public diagnostics redact them.

## Compatibility and migration

Additive port and adapter only; daily-v1 bytes/messages remain unchanged. No
existing objects move. The owner must commit its authoritative receipt/outbox
after storage succeeds, outside the storage operation. Head fencing, admission
durability and orphan reconciliation remain later M03 acquisition work.

## Security and operations

Objects remain owner-private, not directly served by a gateway. No URLs, hashes,
licensed prices or SDK diagnostic sources enter default logging. Backend creation,
credentials, encrypted storage, ACLs and cloud feature activation require separate
review. Expired/revoked objects are unavailable even while bytes still exist.
M03-PR12 owns physical deletion, orphan cleanup and backup/retention policies.

## Validation

`cargo xtask verify --check evidence-storage-contracts` discovers and runs an exact
six-test census. Synthetic proofs cover concurrent create, correction append,
uncertain completion and dropped operations, corruption and stream bounds,
in-flight revocation/expiry, local reopen/scoped keys and diagnostic redaction.
`cargo xtask verify` also enforces dependency boundaries and census mutation tests.
