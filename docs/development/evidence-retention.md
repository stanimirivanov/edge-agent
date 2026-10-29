# Evidence retirement and cleanup

## TL;DR

- Use `RetainedEvidenceStorage` for inventoried writes and current durable denial checks.
- Commit expiry/revocation denial before deleting; never compensate an uncertain PUT.
- Retired request IDs remain denied; erase licensed private metadata and fingerprints.
- Unknown calls leave an opaque account block; clear only an exact confirmed generation.
- Physical cleanup requires independently confirmed write quiescence and is retryable.
- No provider worker, cloud deletion job or licensed backup profile is activated.

## Composition and ownership

The application owns `EvidenceRetention`, `EvidenceDeletion`, `CleanupAuthority`,
`RetainedEvidenceStorage` and `cleanup_evidence`. PostgreSQL owns the inventory,
scope withdrawals, request denials and accounting controls. `ImmutableEvidenceStore`
still owns bounded integrity I/O and now implements exact-key idempotent deletion.
Use explicit backend, pool and timeout configuration; there is no permissive
maintenance authority or automatic scheduler.

For writes, the managed adapter inventories an exact object before PUT and checks
durable denial again after confirmation. For reads, it checks before and after
storage I/O and repeats current local access authorization before releasing bytes.
PostgreSQL publication also requires that descriptor's inventory. A database
constraint binds each deletion descriptor to its immutable key, preventing an
inconsistent inventory row from redirecting cleanup. No hash or receipt
bypasses current authenticated actor/rights checks. M03-PR13 must compose this adapter
and install authenticated rights/withdrawals before enabling intake.

## Denial and retirement

`revoke_evidence_scope` irreversibly withdraws a storage/audience/logical-rights
namespace across revisions. Commit is the local durable denial boundary. Matching
admission, publication, recovery and managed storage operations subsequently deny;
already-returned bytes cannot be recalled. Regranting requires a new rights identity.
This does not promise instantaneous visibility across independent databases/caches.

`retire_due_evidence(1..=16)` processes one transaction per command in deterministic
scope/account/request lock order. Ready evidence is due at its original exclusive
licensed deadline or withdrawal. Work without a receipt becomes orphan-eligible only
at its original command deadline. A live receipt is never an absence-of-use orphan.

Retirement atomically keeps an opaque request-ID denial and removes admission,
command accounting, receipt, owner-private inbox/outbox envelopes and publication
ownership. Research evidence correlations participate in the same owner retirement:
private admission/result and correlated inbox/outbox envelopes are erased, leaving
only an opaque caller-scope/request denial that prevents rebinding. See
[research acquisition](research-acquisition.md). The head belongs to the last publishing request; retiring an older
snapshot cannot remove a newer head. Observation-version fingerprints remain only
while another retained publication owns them. Erasure removes historical receipt
replay and eventually resets a forgotten correction lane; no future correction may
pretend to verify erased history. References are not perpetual audit retention.

An unresolved provider permit becomes an opaque account block with an incremented
generation while its request fingerprint is removed. Existing rate/delay controls
are unchanged. `account_block` reports the generation, not proof of quiescence.
`confirm_retired_account` requires trusted confirmation of all older provider work
and updates only that exact generation. Stale confirmation cannot clear a newer
block. Deleted requests never revive, and clearing a block never refunds calls.
For accounts with [reviewed daily credits](acquisition-ledger.md#reviewed-daily-credits),
confirmation also closes the whole confirmation-day allocation in that transaction:
retirement erased the endpoint price, so a delayed remote debit must not be guessed
away. A stale generation rolls back this charge, and an identical already-cleared
confirmation cannot charge a later day again. Rate/retry history remains unchanged.

## Physical cleanup and recovery

`cleanup_evidence` takes an explicit batch limit and trusted maintenance authority.
It requires owner-confirmed storage-write quiescence per request before deletion;
fence old workers and outstanding backend operations, not just their timers.
It deletes at most twelve exact immutable keys per request outside SQL, then removes
the deletion inventory after all confirmations. A missing key is already deleted.

A denied owner, backend failure, timeout or dropped future leaves the inventory for
retry. Exact-descriptor authorization is required in addition to write quiescence.
Selected requests rotate durably to the back of the queue. Cleanup continues with
independent requests after a failure and returns the first error after the batch;
an unfenced writer must not starve unrelated erasure. Reconstruct adapters and
rerun the bounded batch. Partial progress can include
already-deleted keys; repeated deletion is harmless. No background recovery runs in
`Drop`. Never remove inventory to hide overdue deletion or clear an account just
because a retention deadline passed. Inventory contains licensed deletion identifiers
only while necessary for erasure; it grants no read/use permission.

Bound database pool acquisition separately at composition. Transactions have the
existing five-second statement and one-second lock limits; object operations have
an explicit maximum fifteen-second timeout. The batch has no unbounded retry loop.
Observe bounded counts of denied operations, blocked accounts, pending inventories,
failed/overdue cleanup and confirmed deletion; never label metrics with hashes,
request IDs, scope IDs or object keys. Runtime export/scheduling is PR13 work.

## Migration, backup and rollback

Migration 0004 adds one exact accepted-native preparation per admission, inventoried
before PUT and recovered under current denial/access checks. Admission retirement
cascades its licensed byte erasure; the independent exact-key deletion inventory
survives until quiescent cleanup. See [native persistence](native-daily-acceptance.md#persistence-recovery-and-retirement).

Apply `market-data-postgres::MIGRATIONS` in order with inbox/outbox tables in the same
private schema. Migration 0003 backfills canonical admission retention indexes. It
refuses nonempty legacy head/version tables: stop ingress, inventory/export legacy
ownership and untracked object keys, and review the migration rather than dropping
data. The current unactivated service has no licensed production data. Do not run
new publication with old migrations, or reactivate an older binary after erasure.

The initial local qualification prohibits licensed backups, snapshots and object
versioning. SQL deletion does not scrub WAL, database pages, filesystems or media.
Cloud/versioned/WORM backends need a qualified deletion implementation and SLA,
including encryption/key ownership, backups and restore. Keep restored ingress
closed until current independent denial/rights state is re-established and erased
evidence reconciled; restoring an old DB must never implicitly restore permission.
Quarantine, broker/downstream copies and their histories need separate qualified
retention before licensed runtime activation. This slice does not remotely erase them.

## Verification

```text
cargo xtask verify --check evidence-retention-contracts
cargo xtask verify --check evidence-storage-contracts
cargo xtask verify --check evidence-retention-postgres
cargo xtask verify --check acquisition-terminal-postgres
cargo xtask verify sqlx-metadata
cargo xtask verify
```

PostgreSQL proofs require the owned local `EDGEAGENT_POSTGRES_URL`; SQLx freshness
uses the [isolated metadata workflow](sqlx-metadata.md). Synthetic proofs cover
revocation at I/O boundaries, conservative deletion failure, scoped idempotent
DELETE, orphan/live separation, private metadata erasure, transaction rollback,
restart and stale account confirmations. Protected censuses reject removed,
renamed, duplicated or extra proofs. See
[ADR-0053](../decisions/0053-retire-evidence-with-opaque-denials-and-quiescent-cleanup.md).
