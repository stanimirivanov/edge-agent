# ADR-0073: Bind native stream companions to publication receipts

- Status: Accepted
- Date: 2026-10-28
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Preserve typed native provenance and complete owner inputs in an immutable companion.
- Bind both objects through the existing preparation, receipt, checkpoint and outbox.
- Share deadline, denial and retryable cleanup; retain legacy preparation compatibility.
- Remain offline; PR20C2C still owns runtime, restore composition and readiness.

## Context

Canonical records omit original IEX condition ordering, exchange/tape, provider IDs
and reported count/VWAP. PR20C1 retains these transiently; PR20C2A authenticates full
inputs. Neither persists native provenance with the published capture. A separate
native publication would create a second atomicity and retention problem.

## Decision

Add `edgeagent.evidence.stream-companion.v1`: canonical sorted JSON plus LF with
exact SHA-256, explicit positive byte/record limits and existing portable ceilings
of 4 MiB/4096 records. Preserve full owner-input bytes and typed native metadata at
every canonical position, including original fragment range. No raw frames or
credentials enter the format. The HTTP adapter projects sealed accepted observations
and checks every canonical field with the shared capture encoder. Persistence uses
neutral contracts. Neither a digest nor a DTO grants independent approval.

Bind the exact canonical descriptor and deletion deadline. Attachment requires full
valid owner inputs, the exact sealed session commitment and one-to-one record binding.
Native IDs/minute buckets match canonical identity low bits, and each record uses
an owner-claimed high namespace; direct callers cannot bypass those invariants.
The shared deadline cannot exceed owner-input expiry. Select a shorter canonical
deadline before freezing if needed; never silently edit frozen bytes or renew approval.

Versioned private V2 preparations retain both byte sets and the original opaque event;
V2 descriptors inventory both objects before PUT. Verify both create-only writes
outside SQL, then commit the existing receipt/checkpoint/outbox transaction. Stage
and commit recheck actual independent owner installation through the protected loader.
Validate stored object inventory against the exact preparation when committing,
recovering a receipt and resolving published objects. No new table, query signature,
public event or readiness grant is necessary.

Paired reads return both values and recheck durable controls after I/O. Even the
canonical-only reader verifies its receipt-bound native partner. Legacy stores deny
native operations. Retirement erases the single preparation/event/outbox; paired
descriptors survive until both exact objects are deleted under independently confirmed
writer quiescence. Partial/uncertain deletion retains the complete pair for retry.
Failed publication never compensates by deleting or allocating another identity.

## Alternatives considered

- Drop native fields: loses reproducible evidence meaning.
- Archive raw payloads: expands licensed storage and sensitive-data exposure.
- Separate native receipt/event: adds another consistency and cleanup problem.
- New SQL tables: existing exact preparations and descriptors already bind the pair.
- Force all legacy stores to support companions: unnecessary source break; deny instead.

## Consequences

Pairs add bounded I/O and larger private recovery records. The decoder bounds the
two-artifact JSON-byte-array encoding before parsing. Receipt recovery skips both
PUTs; uncertainty retains exact original IDs. Cleanup counts pairs, not physical objects.
Native metadata establishes neither source continuity nor fill eligibility/readiness.

## Compatibility and migration

Legacy three-field preparations and single-object descriptors retain their bytes and
remain readable. V2 requires the new binary: upgrade consumers before emitting it.
No database migration or lockfile change is needed. `StreamCleanup` gains an optional
companion field; legacy source callers supply `None`. Default storage denial and
ledger absence preserve existing implementations. Rollback must stop writers and
use a compatible binary to recover/retire V2 work while preserving object inventory;
old binaries cannot parse V2. Never erase namespace tombstones during rollback.

## Security and operations

Use the same canonical principal/account/source/instrument/audience/storage/rights and
exclusive deadline, in a distinct `stream-companion-v1` namespace. Daily cleanup cannot
enter it; hashes/descriptors cannot authorize access. Current authority is rechecked
during bounded reads and after effects. Only the independent owner retires and
confirms storage writers quiescent; timeouts, cancellation and death do not suffice.
Retirement never releases provider capacity or resets usage. No provider/model calls,
activated worker mode or live broker path ships.

## Validation

Pure proofs cover canonical encoding, owner/session and record binding, bounds,
redaction and legacy/V2 recovery. IEX trade/bar proofs check exact native projection.
Memory-store proofs cover uncertain creation, collisions, denial/expiry and paired
cleanup. The isolated PostgreSQL proof covers independent approval, failed second
PUT without receipt/outbox, changed retries, exact recovery, committed replay, missing
native reads, revocation and partial cleanup inventory. Protected inventories and
task routes include these proofs. Local compilation/strict Clippy run; heavy
database/storage/conformance and default acceptance run in CI.
See [native streaming provenance](../development/stream-native-provenance.md).
