# Immutable streaming capture segments

## TL;DR

- PR17A freezes normalized capture, not readiness or a durable checkpoint.
- Explicit segment limits supplement PR16 frame/account budgets; no defaults.
- Store exact create-only bytes in a separate licensed namespace.
- Retry uncertainty with the same identity; never repair or compensate by deletion.
- Durable inventory, capture checkpoints/outbox and scoped retirement precede runtime use.

## Capture and encoding

PR20C2B adds [receipt-bound native companions](stream-native-provenance.md). New paired
preparations inventory both objects and share the canonical deadline, denial and cleanup;
legacy canonical-only encoding remains readable. Native metadata never grants readiness.

`StreamSegment` owns `edgeagent.evidence.stream-segment.v1`: compact recursively
sorted JSON plus one LF; SHA-256 includes LF. Decode bounds before parsing, validates
fields and rejects alternate bytes, duplicate/unknown keys and unsupported formats.
Explicit byte/record limits have portable ceilings of 4 MiB/4096, not recommended
operating defaults. Segments never bypass the bus envelope limit.

One segment covers one selected instrument, full principal/account/source/endpoint/
audience/storage/rights, exact session digest, semantics, connection generation,
explicit feed epoch, inclusive frame range and prior digest. Retain/authenticate the
complete session, selection, contract and reviews; hashes are not authority.
Source sequences (exact u64 text including zero) differ from session-local frame/item
positions. Positions preserve gaps after filtering. Controls can yield no records.
No segment proves continuity, historical availability, freshness or simulation eligibility.
Epochs/generations/prior digests prove neither ownership nor a durable chain head.

Values preserve trade USD/share and shares, explicit raw core/intraday OHLCV,
non-tradable index points, option sides/specification and optional displayed sizes.
Explicit cancellations carry predecessors, not invented zero prices. Repeated source
facts at distinct arrivals preserve positions/receipts; conflicting same-revision facts
within a segment fail closed. Domain observations must match session source, contract,
delivery, instrument and actual receipt window. Codec checks cannot authenticate global
histories or specifications. No raw provider frames or credentials are stored.

## Authority, storage and failure

`StreamSegmentStorage` and trusted `StreamSegmentAccess` belong to the application.
Access resolves authenticated actor, full retained session/selection/reviews, current
rights, original retention and durable denial. A descriptor/digest cannot authorize
itself. No permissive product implementation or live composition is supplied.

`ImmutableEvidenceStore` reuses its explicit backend and positive whole-operation
timeout (at most 15 seconds). Checked numeric/hex path fragments form:

```text
evidence/stream-segment-v1/{storage}/{audience}/{rights-id}/{rights-revision}/{session}/{sha256}.json
```

Full bytes bind remaining scope. Never globally deduplicate by hash. Daily readers
and retirement cannot enter this namespace; existing daily behavior is unchanged.
Conditional create always requires exact verified read-back, including existing keys.
GET checks metadata/full range, bounds each chunk before extending, verifies length,
digest/canonical encoding/full descriptor, and repeats authority/exclusive expiry
during and after I/O. SDK transport buffers need separate qualification.
Diagnostics exclude payloads/scopes; source chains require deliberate redaction.

Freezing caps explicit deletion by earliest actual receipt plus licensed canonical
lifetime and agreement deadline, conservatively rounded down to milliseconds.
Empty control-only capture anchors retention at capture time. Shorter deadlines
are allowed; later freezing cannot renew populated capture's original retention.
Trusted access must enforce original rights on reconstructed writes/reads.

Timeouts or dropped futures may leave objects. Retry exact bytes/key under current
authority; never overwrite collisions, choose replacement identities or compensate
by deleting. `publish_stream_segment` inventories before PUT, verifies charged
ranges/current generations and serializes chain/checkpoint/receipt/outbox. Recover
the original preparation with `pending_segment` (session/instrument discovery) or
`recover_segment` (known opaque ID); never refreeze it after a restart.
Committed result recovery skips PUT. Notifications contain only opaque IDs.

Apply migration 0008 after 0007. Legacy active generations have no known starting
ordinal: publication fails closed until independently confirmed connection quiescence
and a fresh reservation. Never reset counters or release slots during migration.
Provision `edgeagent_stream_retention` through the independent owner only; workers
receive SELECT and EXECUTE on `edgeagent_lock_stream_retention(TEXT)`, never control
DML. Other publication/head tables use trusted application worker DML.

Cancellation stops new capture/publication, not previously published reads. Current
exact rights and original expiry still govern retained access beyond the capture
window. `withdraw_stream_data` immediately denies reads/publication. The read
coordinator checks durable denial before and after bounded object I/O.

Retire expired or withdrawn published objects, or unpublished preparations from stopped
sessions. Stop alone cannot retire unexpired published objects. Retirement atomically
erases private preparation/receipt/head digest/outbox and retains a deletion descriptor.
Active lanes retain a hash-free capture position: retiring history cannot spend
charged frames again. Unpublished uncertain work holds its lane until capture stops.
Once stopped with every segment retired, erase lane metadata and full admission.
`confirm_stream_writers_quiescent` is an independent irreversible owner assertion
about outstanding PUTs; it stops writers but does not free provider connection slots.
`cleanup_stream_segments` handles 1..16 exact descriptors outside SQL and requires
independent per-object confirmation before and after deletion. Unknown deletions
retain inventory. Final cleanup erases licensed admission witnesses while preserving
opaque denial, usage and unresolved capacity. Deadlines never prove quiescence.
Successor fingerprints cannot outlive their predecessor's licensed deadline. Cleanup
preserves opaque intent tombstones to prevent reuse. Checkpoint flags distinguish
first capture, retired history, reconnect/epoch and skipped frames; none establishes
source continuity or readiness.

## Verification and rollout

Run `cargo xtask verify --check streaming-segment-contracts` then `cargo xtask verify`.
Also run `cargo xtask verify --check streaming-publication-postgres` with the isolated
local PostgreSQL fixture. The protected census covers pure freezing/codec and synthetic memory-store integrity,
isolation, denial/expiry and committed-write confirmation loss. It does not qualify
cloud storage, vendors, fsync, physical disconnect or process-death recovery.

No dependency, daemon mode, credentials or live calls are added.
Rollback stops new workers and retains additive tables, uncertain objects and
unresolved sessions. Daily objects are unchanged.
PR18 transport, PR19 adapter and PR20
supervision remain separate. Daily live qualification remains explicitly not run.
