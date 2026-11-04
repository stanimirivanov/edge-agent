# Audited Research kill switches

## TL;DR

- Disable a caller scope or exact implementation to stop every fresh completion.
- Submit a consecutive, audited operator revision; enable never overrides another disable.
- Keep historical receipts, lifecycle and committed outbox delivery available.

[ADR-0102](../decisions/0102-fence-fresh-research-with-audited-kill-switches.md) extends
[validated publication](validated-publication.md). Switches govern publication-time
admission, not a historical decision cutoff, policy eligibility or account-risk authority.
A scope switch covers all its implementations; an implementation switch covers all
configurations of that digest. Use the scope switch to cover future implementations.

## Operator command and authority

Apply [migration 0005](../../crates/research-postgres/migrations/0005_research_controls.sql)
after Research 0001–0004 in the same private schema, with intake stopped. Existing rows
retain explicit pre-control absence. No rows are synthesized as operator actions.

With the existing loopback database, Research schema and explicit scope settings,
run `edgeagent-research offline-control` with canonical bytes on closed stdin. For
example, the exact one-line command below (including final LF) disables scope operator:

```json
{"data":{"enabled":false,"reason_code":"maintenance","revision":1,"scope":"operator","target":{"kind":"scope"}},"kind":"research.control.v1"}
```

For an implementation, use target `{"digest":"<64 lowercase nonzero hex digits>","kind":"implementation"}`.
The placeholder must be replaced by the actual approved implementation digest. Re-enable
with enabled=true and the next revision for the same target, with an explicit reason
code such as reviewed-resume. Both targets must be enabled. No recorded action means
enabled; a missing/unreadable control table fails closed, rather than implying absence.

Commands are bounded to 2,048 bytes, reject unknown fields/versions, and require exact
sorted-key canonical JSON plus final LF. Reason codes are 1–64 ASCII alphanumeric,
period, underscore or hyphen characters; do not put private free text in them. Revision
starts at one, is consecutive per scope/target and fits a positive PostgreSQL bigint.
Exact latest same-actor retries are idempotent. A stale disable/enable cannot overwrite
a later action. An initial explicit enable is permitted and audited.

Only PostgreSQL session_user owning the current schema may administer. Actor and time
come from the session and transaction, never the command. The CLI also refuses a body
scope different from its configured scope. No owner evidence binary, strategy approval
or broker connection is needed for administration. Local owner authority is the existing
[P3 model](strategy-governance.md), not production multi-user separation of duties.
Grant runtime SELECT on controls, with existing completion/decision/inbox/outbox rights;
do not grant control writes, DDL, TRUNCATE or trigger-disable rights to workers.

## Transaction, recovery and audit

Administration acquires the exclusive scope advisory lock. Pre-read approval/control
checks and fresh completion acquire the shared lock, held through the transaction.
Disabling first prevents new completion, decision, inbox and outbox effects. A completion
already holding the lock commits before the disable can commit. Recheck at commit
closes the owner-I/O/computation window; external calls stay outside transactions.

The private immutable decisions row retains scope_control_revision and
implementation_control_revision. Zero denotes the enabled no-recorded-switch default
actually checked under the lock. Null denotes historical absence before this migration;
these meanings are not interchangeable. Referenced nonzero enabling actions are rechecked
on receipt reads without consulting current switches. Keep their immutable actor, reason,
revision and occurred_at history for the entire completion retention period.

A deferred constraint requires snapshots for all new rows. Older binaries cannot silently
publish unfenced completions after migration. The existing artifact and decision wire
bytes remain unchanged; projections are observations, not proof of current switch state.

## Pausing and resuming work

Disabled is an administrative refusal, not an evidence abstention or malformed message.
The broker worker retries after 15 seconds and does not acknowledge, record successful
inbox/progress or quarantine the valid delivery merely because a switch is disabled.
At the configured three-delivery ceiling it remains unresolved in the retained stream.
After an audited resume, use the reviewed consumer-reset/replay procedure if the delivery
budget was exhausted. Resume does not automatically reset consumers or change IDs.

Previously committed outbox events still drain, preserving the atomic publication promise.
Exact receipts recover while disabled without new owner reads or permission for new
computation. Lifecycle actions and read-only replay remain available. To withdraw an
existing artifact, use its separate [owner lifecycle](research-lifecycle.md); switches do
not rewrite historical status or bypass entitlement and current approval checks.

Rollback stops intake and preserves control history, snapshots and constraints for a
compatible binary. Never erase disables or remove the database guard to downgrade.
Configured invalidation uses separate [retained-decision checks](research-invalidation.md); no public HTTP or broker control surface is added.

## Verification

Run `cargo test --locked -p edgeagent-contracts --test research_controls` and
`cargo xtask verify --check research-persistence --check research-worker-postgres --check research-replay-postgres --python python3`
with an isolated loopback PostgreSQL URL. Add an isolated NATS URL for
`cargo xtask verify --check research-worker-broker --check research-processes --python python3`.
Run `make repository architecture PYTHON=python3`, `make verify PYTHON=python3` and the
canonical SQLx metadata gate with its documented tooling. Online macro compilation
is separate evidence and cannot claim the canonical Compose/CLI gate passed.
