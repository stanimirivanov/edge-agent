# Durable native streaming provenance

## TL;DR

- PR20C2B binds immutable native provenance to the canonical publication receipt.
- Explicit limits, scope and deadline govern both objects; approval remains independent.
- Unknown writes retain exact recovery data; partial cleanup retains both descriptors.
- Offline capability only; PR20C2C still owns runtime and readiness composition.

## Freeze, attach and publish

Freeze canonical capture with explicit limits and deadline. Call
`IexCanonicalProfile::companion(segment, inputs, accepted, limits)` with the same
sealed accepted observations in record order. The adapter compares every canonical
field using the shared capture encoder and preserves original trade IDs (including
above 2^53), condition ordering, exchange/tape or reported count/exact VWAP.
Native IDs/minute buckets must match canonical identity low bits; every record's
high namespace must appear in the complete owner-approved claims.
Full owner-input bytes retain complete profile, authority and selection witnesses.
No raw frames, credentials or reconstructed metadata enter the artifact.

Attach using `StreamPublication::with_companion` before staging. The complete sealed
session commitment must match. The canonical deadline cannot exceed the explicit
owner-input expiry: choose a compatible deadline before freezing rather than editing
frozen bytes. Explicit native byte/record limits obey portable 4 MiB/4096 ceilings;
these are not defaults or an expansion of the bus envelope.

The existing publisher inventories the V2 preparation and paired descriptor before
PUT, verifies both exact create-only objects outside SQL, then commits the same
checkpoint/receipt/outbox transaction. Stage/commit load actual protected owner inputs
and recheck current authority/cancellation. Inventory must match the exact preparation
when committing or recovering a receipt and when resolving published objects.

If a write or confirmation fails, recover the original preparation through existing
pending/known-ID APIs. Never refreeze, choose another ID, overwrite or compensate by
deletion. A changed companion conflicts; committed receipt recovery skips both PUTs.
The opaque published event remains unchanged and contains no licensed descriptors.

## Read, deny and clean up

`read_stream_publication` returns canonical capture and optional native companion,
checking exact receipt bindings and durable controls before/after storage I/O.
`read_stream_segment` also verifies the native partner before returning only canonical
values. A legacy `None` establishes no qualified native evidence or readiness.

Companion keys use checked numeric/hex scope fragments:

```text
evidence/stream-companion-v1/{storage}/{audience}/{rights-id}/{rights-revision}/{session}/{sha256}.json
```

Full bytes bind remaining scope and canonical digest. No global hash dedup or bearer
read capability exists. Existing whole-operation timeout, conditional create/read-back,
size/range checks, bounded chunks, canonical decoding and current authority apply.

Owner withdrawal denies both reads. Retirement atomically erases the shared private
preparation/event/outbox and keeps paired deletion descriptors. Existing cleanup
requires independent writer quiescence for the exact pair, deletes outside SQL and
clears inventory only after both objects and confirmation succeed. If one deletion
succeeds and the other fails, retry the original pair; absence is idempotent.
The explicit 1..16 limit counts pairs, at most two objects each. Stop, cancellation,
death and elapsed time never prove quiescence or release provider capacity/counters.

## Compatibility, verification and remaining work

No schema migration or SQLx signature is added. Legacy preparation/descriptor bytes
remain readable; paired private records use explicit V2 markers. Upgrade consumers
first. Rollback needs a compatible binary to recover/retire V2 work while preserving
inventory. Legacy cleanup constructors supply `companion: None`; unsupported stores
deny native I/O and legacy ledgers explicitly report no companion.

CI owns `cargo xtask verify --check streaming-segment-contracts`,
`cargo xtask verify --check iex-canonical-acceptance`,
`cargo xtask verify --check streaming-publication-postgres`, SQLx freshness and
default `cargo xtask verify`. Local compilation and strict Clippy run before delivery;
heavier storage/database/provider-conformance proofs remain assigned to CI.

PR20C2C must compose fresh input/restore checks, bounded offline tasks, exact recovery,
publication and domain readiness. M03/PR20 remain incomplete. No provider activation
or live qualification is included.
See [ADR-0073](../decisions/0073-bind-native-stream-companions-to-publication-receipts.md).
