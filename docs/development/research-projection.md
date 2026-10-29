# Pure Research projection

## TL;DR

- Initialize from an authenticated scoped request; pending does not mean rejected.
- Apply exact owner artifact/abstention records once; conflicting content rejects.
- Treat lifecycle Gap as unresolved input; do not advance consumption progress.
- Persist checkpoint and cursor atomically in the future projector adapter.

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
must never be interpreted as one. A lifecycle record before its base artifact or
requiring the later M04-PR13 transition handler returns Gap. It is not buffered,
consumed or silently skipped. A known wrong artifact reference or lifecycle fact
against an abstention conflicts. The future consumer must retain the failed input,
expose unresolved gap/lag and avoid advancing its cursor; an older view alone must
not be advertised as current. Lifecycle commands/transitions are outside this slice.

`checkpoint` writes a private EARP1 encoding of canonical request and optional
terminal bytes, followed by a 32-byte SHA-256 corruption checksum. Its total limit
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

The [projector runtime](projector-runtime.md) adds persistence/consumer/internal queries; PR12 adds Gateway queries; PR13
adds lifecycle transitions. No schema migration, server, broker provisioning,
provider access or artifact mutation is introduced here. Disposable views can be
removed on rollback and rebuilt from authenticated retained owner facts.
