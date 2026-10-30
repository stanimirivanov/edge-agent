# Pure Research projection

## TL;DR

- Initialize from an authenticated scoped request; pending does not mean rejected.
- Apply exact owner artifact/abstention records once; conflicting content rejects.
- Treat lifecycle Gap as unresolved input; do not advance consumption progress.
- Persist checkpoint, inbox and applied marker atomically in the projector adapter.

[`ResearchProjection`](../../crates/audit-domain/src/research.rs) is independent of
account/order/risk projection state. It has no transport, database, HTTP, clock or
authentication capability. The caller validates owner source/route and establishes
the trusted request anchor. Canonical bytes and checksums alone confer no authority.
An authenticated terminal owner record can supply the initial request independently
of checkpoint storage; untrusted checkpoint content cannot supply its own anchor.

`new` starts pending with local view revision zero. A matching artifact or abstention
advances once to revision one and retains exact canonical Research v1 bytes.
`apply` returns Duplicate for identical retained request/outcome data. Changed intent
under the same scope/request ID or competing terminal outcomes return Conflict;
foreign scope/request returns Scope. Every error leaves view/checkpoint unchanged.
Request status is an observation, not current eligibility, expiry or entitlement.

Research terminal events have no owner revision chain. Local projection revision
must never be interpreted as a global stream offset. [Lifecycle changes](research-lifecycle.md)
append consecutive explicit-time owner revisions without changing outcome bytes;
projection revision becomes one plus that lifecycle revision. Duplicate retained
facts are harmless; missing base/prior revisions return Gap without mutation.
Wrong artifact references, illegal states or conflicting revisions return Conflict.
The adapter keeps unresolved input visible while backfilling predecessors.

`checkpoint` retains EARP1 for empty histories and uses EARP2 for lifecycle histories,
including up to two canonical lifecycle records and the original outcome, followed
by a 32-byte SHA-256 corruption checksum. Its total limit
is `4 * MAX_RESEARCH_BYTES + 1024` bytes, including JSON escaping/framing. Restore
checks size before parsing, checksum, version, exact canonical framing, all embedded
contracts and the independently provided original request. Restored state derives
status/revision from validated facts rather than trusting separate cached flags.
Storage authentication, rollback fencing and atomic checkpoint/cursor persistence
remain adapter obligations. An authorized writer can still roll back or replace a
self-consistent checkpoint; this checksum is not a signature.

Run `cargo xtask verify --check research-projection-contracts --python python3` for
protected pending/replay, duplicate/conflict, lifecycle-gap and checkpoint refusal
proofs with `serde_json/preserve_order`. `cargo test --locked -p edgeagent-audit-domain`
also exercises existing account replay/evaluation. The default gate includes the
focused suite and its exact named discovery. No external service is required.

The [projector runtime](projector-runtime.md) adds persistence/consumer/internal queries; [Gateway queries](gateway-queries.md) expose observations and [lifecycle state](research-lifecycle.md). No schema migration, server, broker provisioning,
provider access or artifact mutation is introduced here. Disposable views can be
removed on rollback and rebuilt from authenticated retained owner facts.
