# Immutable evidence-object storage

## TL;DR

- `market-data-application` owns the storage and current-access ports.
- `market-data-object-store` implements create-only writes and bounded verified reads.
- Exact canonical daily-v1 bytes remain outside messages and inside licensed scopes.
- Retry uncertain writes with the same bytes/key; never overwrite or compensate by deletion.
- Compose durable retirement and exact-key deletion through the retention adapter; no cloud activation or public read API.

## Composition and authority

Construct `ImmutableEvidenceStore` with an explicit `Arc<dyn ObjectStore>` and
positive whole-operation timeout of at most 15 seconds. No default bucket,
credentials, environment discovery, storage path, retries or timeout is selected.
The injected backend must provide atomic conditional creation. Separate tenant
storage roots/buckets, cloud encryption and IAM remain deployment responsibilities.
The constructor does not certify a supplied backend.

Each operation requires a trusted `EvidenceAccess` implementation. It resolves
the authenticated actor and complete retained request/rights records; unknown,
conflicting, expired, withdrawn or mismatched authority denies access. Matching a
hash or caller-asserted descriptor is not authorization. This PR supplies no live
authorization implementation; later worker/read composition must provide one.
The injected clock supplies current UTC, not historical capture time. The adapter
independently enforces the exclusive descriptor deadline and repeats authority
checks after I/O. Retained descriptors do not bypass current revocation.

## Storage and integrity contract

The owner-private key format is:

```text
evidence/daily-v1/{storage}/{audience}/{rights-id}/{rights-revision}/{sha256}.json
```

All segments come from checked IDs/revisions; keys contain no arbitrary paths or
URLs. Do not use global hash-only deduplication. The canonical artifact also binds
principal, provider account, source, instrument, listing, endpoint and request.

Writes use SDK conditional create, not overwrite. Both successful creation and
an existing key require a verified read-back before returning an `EvidenceObject`.
Compare exact bytes, full length, SHA-256, canonical encoding and all descriptor
fields, including retention and predecessor. A conflicting/corrupt object fails
closed and is never repaired in place. Operator diagnosis must not log its bytes.
Corrections create independent objects and preserve the predecessor; this adapter
does not decide which concurrent correction becomes the authoritative head.

Downloads reject oversized/mismatched metadata and non-full ranges, check each
chunk before extending the accumulator, and reject short, long, noncanonical or
tampered content. The ceiling remains 4 MiB. SDK transport buffers are outside
the accumulator guarantee; a future cloud profile must qualify its transport
limits, retries, conditional-write semantics and encryption independently.

## Failure, recovery and lifecycle

`Invalid`, `Denied`, `Expired`, `NotFound` and `Integrity` are explicit outcomes.
`Backend` preserves the real source error without rendering it; avoid automatic
error-chain logging because SDK errors may include private keys/URLs. `Deadline`
means the operation exceeded its configured whole-operation timeout.

A failed, cancelled or timed-out write may already exist. Reconstruct the adapter
and retry the same canonical artifact; create-only plus read-back confirms the
same object without overwriting it. Dropping a future performs no deletion or
background recovery. Publication is forbidden until successful storage and the
owner's atomic receipt/outbox commit. Storage I/O never belongs inside SQL
transactions. A failure after object creation can leave an orphan for later
reconciliation; it must not cause an uncommitted ready event.

Expiry/revocation denies use immediately; neither implies physical deletion.
The [retention boundary](evidence-retention.md) implements durable denial, bounded
orphan cleanup and scoped deletion after write quiescence. Runtime still owns
authenticated policy composition, scheduling, and provider/process fencing.
No raw response storage or live provider access is enabled here. Rollback can stop
using the adapter without changing existing immutable evidence or v1 contracts.

## Verification and portability

```text
cargo xtask verify --check evidence-storage-contracts
cargo xtask verify
cargo xtask verify supply-chain
```

The default gate protects seven named tests with an independent census and mutation
tests. They cover concurrent creates/corrections, a committed PUT followed by
confirmation timeout and restart, dropped operations, corrupt collisions,
descriptor and bounded-stream attacks, current revocation/exclusive expiry during
I/O, local-store reopening, licensed namespace isolation and diagnostic redaction.
Tests use only invented repository evidence, memory and owned temporary files.
Virtual-time throttling models I/O boundaries without sleep-based coordination.

`object_store` 0.14.2 is MIT/Apache-2.0; only its filesystem feature is enabled.
It provides cloud implementations, but this change neither builds nor qualifies
them. Local reopening proves persistence across adapter reconstruction, not fsync,
power-loss, multi-machine, WORM or cloud acceptance. See
[ADR-0047](../decisions/0047-store-scoped-evidence-with-create-only-integrity-checks.md)
and [evidence bytes/messages](evidence-artifacts.md).
