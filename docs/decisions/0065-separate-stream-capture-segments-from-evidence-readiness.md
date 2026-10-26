# ADR-0065: Separate stream capture segments from evidence readiness

- Status: Accepted
- Date: 2026-10-26
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use a bounded normalized streaming format and licensed storage namespace.
- Bind finite admission, explicit epoch/generation, capture positions and retention.
- Stored capture is not evidence readiness or durable publication.
- PR17A stays offline; PR17B supplies checkpoint/outbox and retention composition.

## Context

PR16 charges frames but retains no payload and proves no continuity. Daily formats
and retirement describe terminal accepted daily acquisitions. Reusing their meaning
or cleanup would misrepresent streaming capture. The maintainer approved splitting
PR17 into segment/storage and durable publication.

## Decision

Introduce `edgeagent.evidence.stream-segment.v1`: canonical sorted JSON plus LF,
SHA-256, explicit operator limits capped at 4 MiB/4096 records. Bind complete licensed
scope and exact session digest (including selection/configuration/reviews), semantics,
delivery, independent feed epoch, connection generation, local frame range and prior
segment digest. Preserve normalized values, source revisions/cancellations/sequences,
event/provider/receipt times, local session label and qualified conditions.
Empty control-only segments are legal capture, not usable market evidence.

The pure producer requires checked domain observations with exact session source,
instrument/contract/delivery and receipts within its window. Recheck current pinned
authority and cancellation. Explicit deletion cannot exceed earliest actual receipt
plus licensed canonical lifetime or agreement deadline; round down, never extend.
Full authenticated session/reference histories remain required, not only checksums.

Application-owned storage/access ports reuse the bounded conditional-create adapter
with a distinct namespace/decoder. Verify exact read-back and full descriptor; bound
downloads and recheck current authority/exclusive expiry before/after I/O. Unknown
writes retry identical bytes/key. No overwrite or compensating deletion is allowed.

PR17B must verify charged ranges/current generations, inventory before PUT, serialize
checkpoint/chain/outbox commit, and integrate durable denial, scoped retirement and
quiescent orphan cleanup. PR17A grants no readiness, live authority or runtime mode.

## Alternatives considered

- Daily-v1 reuse: rejected; accepted whole-series/terminal meaning differs.
- Raw archive: rejected without separately reviewed raw rights and transport bounds.
- Bytes on the bus: rejected by envelope limits and data minimization.
- Continuity from local frames: rejected; controls/filtering and source sequencing differ.
- Combined persistence/runtime PR: rejected in favor of the approved split.

## Consequences

Capture/storage can be tested independently without PostgreSQL, transport or vendors.
Mappings cover admitted value classes without new dependencies. Dedicated ports avoid
accidental daily reads/cleanup without speculative generic repositories. Codec checks
cannot authenticate source truth, specifications or global revision histories.

## Compatibility and migration

Additive library format/API only. Daily bytes/messages/metadata and SQL schema remain
unchanged. Preserve v1 meaning; use a new version for incompatible evolution. Rollback
must not mutate stored bytes or release unresolved session markers.

## Security and operations

Trusted access authenticates actor, complete session/selection, current rights,
original retention and durable denial. Constructors/digests grant no permission.
Payloads/namespace diagnostics remain private. Backend IAM/encryption/conditional
creation and SDK buffers require qualification. Licensed activation remains blocked
pending durable inventory/retirement and independent quiescence confirmation.

## Validation

`cargo xtask verify --check streaming-segment-contracts` protects synthetic producer,
codec and memory-store tests with independent census mutations and route regression.
`cargo xtask verify` covers workspace compilation, linting, tests, docs and harness.
Proofs reject changed authority/bindings/limits, malformed bytes and corrupt collisions,
preserve source sequencing, and recover committed PUT confirmation loss.
