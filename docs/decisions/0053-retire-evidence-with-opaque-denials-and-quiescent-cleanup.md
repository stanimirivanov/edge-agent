# ADR-0053: Retire evidence with opaque denials and quiescent cleanup

- Status: Accepted
- Date: 2026-10-23
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Commit denial before physical deletion. Erase licensed admission, receipts and
fingerprints while retaining opaque request tombstones and account safety controls.
Unknown provider calls keep the account blocked until generation-fenced,
owner-confirmed quiescence. Delayed storage writes require separate quiescence.

## Context

Immutable PUTs can commit without confirmation; publication can fail after storage.
Deleting an apparent orphan while its writer remains active can resurrect bytes.
Account pending permits contain request fingerprints. Keeping them indefinitely
violates retention, but deleting them without replacing the fence permits new calls.
The maintainer approved opaque denials and continued account blocking after erasure.

## Decision

Keep the application-owned `EvidenceRetention`, `EvidenceDeletion` and
`CleanupAuthority` boundaries. Compose `RetainedEvidenceStorage` around the existing
integrity adapter. Inventory exact descriptors before PUT; recheck durable denial
before reads and after I/O, independently of current authenticated rights. PostgreSQL
ready publication requires an inventoried descriptor, so an unmanaged PUT cannot
publish. Hashes and retained receipts remain non-authorizing.

Serialize in scope/account/request order. Irreversible namespace withdrawal covers
storage, audience and logical rights ID across all revisions. Shared scope locks
serialize admission/publication against revocation and retirement. Revocation commits
without waiting for quiescence or deletion; the next durable check denies use. This
does not recall bytes already returned or promise synchronous global propagation.

Retire expired/revoked ready evidence, and unpublished work only after its original
command deadline. Each bounded batch has at most sixteen commands and twelve objects
per command. In one transaction, retain a request-ID-only denial; erase admission,
command budget, receipt, owner-private inbox/outbox copies and publication ownership.
Erase the current head when its owning request retires; erase observation fingerprints
only when no retained owner still requires them. Keep scoped deletion identifiers
only until confirmed physical deletion, never as an evidence-use entitlement.

When retiring an unresolved permit, erase its content fingerprint and increment an
opaque account block generation in the same transaction. Keep rate/delay accounting.
Only trusted confirmation for that exact generation clears the block. Old command
recovery stays denied, including after unblocking. Never infer quiescence from a
lease, timeout, deadline, cancelled future or remote request uncertainty.

Physical deletion runs outside SQL after owner-confirmed storage-write quiescence.
Require exact-descriptor deletion authorization as well. Rotate selected requests
durably and continue independent work after failure, reporting the first error;
unfenced old writers cannot pin the cleanup queue.
Retry the same inventory after uncertain DELETE; absent keys count as success.
Only then erase the deletion inventory. The trusted composition supplies maintenance
authority; neither a public request nor a permissive default does so.

## Alternatives considered

- Delete pending permits and allow new work: loses the account uncertainty fence.
- Keep fingerprints forever: contradicts licensed metadata retention.
- Delete immediately after PUT failure or lease expiry: races delayed writes/publication.
- Retain historical receipts as read grants: bypasses revocation and expiry.
- Activate cloud lifecycle rules now: cannot qualify backup/version/media erasure locally.

## Consequences

Safety can require an account to remain unavailable until the owner fences old work.
Physical erasure can remain outstanding; inventory counts and overdue work must be
operationally visible. Retirement intentionally removes historical receipt replay.
After a current head is erased, a future acquisition begins a new retained lineage;
it cannot validate a correction against a forgotten predecessor. No perpetual
immutable-version or post-erasure reproducibility guarantee is made.

## Compatibility and migration

Daily-v1 wire contracts and object keys do not change. Apply migration 0003 before
new adapters; it backfills admission retention indexes but refuses legacy nonempty
head/version tables because their ownership cannot be recovered from hashes alone.
Stop intake and inventory/export legacy publication metadata and pre-inventory objects
before migration; never drop it automatically. No product worker is active yet.
Old binaries cannot enforce tombstones: do not roll them back into active service.

## Security and operations

Opaque denial IDs, scopes and block generations are controls, not licensed content
fingerprints. Default diagnostics redact deletion descriptors. Storage/admin ports
are trusted owner interfaces, not authenticated HTTP APIs. Restore must keep ingress
closed until independent denial/rights state is re-established and cleanup completes.
Licensed backups, snapshots, object versioning and WORM are prohibited in the initial
local profile; cloud activation requires a separately qualified erasure/restore plan.
SQL DELETE is logical erasure, not WAL/vacuum/media sanitization. Downstream copies
require their own current-access gate and retention qualification before activation.

## Validation

`cargo xtask verify --check evidence-retention-contracts` protects the synthetic
I/O and cleanup coordination proofs; `evidence-storage-contracts` adds idempotent,
scoped deletion/timeout recovery. `evidence-retention-postgres` proves denial,
orphan/live separation, metadata erasure, rollback and account generation fencing in
owned schemas. `cargo xtask verify sqlx-metadata` checks real schema descriptions;
`cargo xtask verify` includes the default sensors and independent census mutations.
These are not cloud, process-fencing, backup or physical-media erasure proofs.
