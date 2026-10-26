# Immutable streaming capture segments

## TL;DR

- PR17A freezes normalized capture, not readiness or a durable checkpoint.
- Explicit segment limits supplement PR16 frame/account budgets; no defaults.
- Store exact create-only bytes in a separate licensed namespace.
- Retry uncertainty with the same identity; never repair or compensate by deletion.
- PR17B integrates inventory, retirement and checkpoint/outbox before runtime use.

## Capture and encoding

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
by deleting. PR17A has no inventory, checkpoint, outbox, cleanup grant or runtime.
PR17B must inventory before PUT, verify charged ranges/current generations,
serialize chain/checkpoint/outbox, integrate durable denial and erase scoped metadata
only after independent write quiescence. Deadlines do not imply quiescence.
Licensed activation remains blocked pending these lifecycle and restore gates.

## Verification and rollout

Run `cargo xtask verify --check streaming-segment-contracts` then `cargo xtask verify`.
The protected census covers pure freezing/codec and synthetic memory-store integrity,
isolation, denial/expiry and committed-write confirmation loss. It does not qualify
cloud storage, vendors, fsync, physical disconnect or process-death recovery.

No dependency, SQL migration, daemon mode, credentials or live calls are added.
Rollback stops using the API without touching daily objects or unresolved sessions.
PR17B owns durable publication/retirement; PR18 transport, PR19 adapter and PR20
supervision remain separate. Daily live qualification remains explicitly not run.
