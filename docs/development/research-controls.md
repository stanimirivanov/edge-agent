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
Use the single-message recovery procedure below when the delivery budget is exhausted.
Resume does not automatically reset consumers or change IDs.

Previously committed outbox events still drain, preserving the atomic publication promise.
Exact receipts recover while disabled without new owner reads or permission for new
computation. Lifecycle actions and read-only replay remain available. To withdraw an
existing artifact, use its separate [owner lifecycle](research-lifecycle.md); switches do
not rewrite historical status or bypass entitlement and current approval checks.

Rollback stops intake and preserves control history, snapshots and constraints for a
compatible binary. Never erase disables or remove the database guard to downgrade.
Configured invalidation uses separate [retained-decision checks](research-invalidation.md); no public HTTP or broker control surface is added.

## Recover one exhausted delivery

This procedure applies to the trusted local operator profile. It uses the existing
`offline-once` entry point and keeps the exhausted consumer intact. The broker proof
uses an invented owner process, actual PostgreSQL/JetStream and the actual Research
binary. It qualifies processing and retained identity, with current production owner
rights still checked by the installed owner adapter.

1. Stop Research intake for the affected scope and identify the exact broker account,
   stream, consumer and exhausted **stream sequence** from retained delivery/advisory
   evidence. Record a private recovery ticket with the scope, request ID, message
   source/ID and the independently verified reason for resumption. Consumer sequence
   is a different counter. Never select the latest message on the subject.
2. With a read-authorized operator identity, request the exact message through
   JetStream `$JS.API.STREAM.MSG.GET.<stream>` with JSON `{"seq":<sequence>}`.
   The existing Rust client exposes this as `stream.get_raw_message(sequence)`.
   Use a finite timeout and cap the encoded API response at 400000 bytes.
   After base64 decoding, enforce the existing 262144-byte envelope ceiling. Authenticate the account/stream separately. Confirm the returned
   sequence and subject, then decode the response's base64 data into a private file.
   Preserve the exact envelope bytes, including all correlation and causation fields.
   Missing, retired, mismatched or unverifiable evidence stops recovery.
3. Validate the envelope and its Research request against the recovery ticket. Keep
   evidence files and broker responses outside Git and logs under the applicable
   licensed retention policy. Grant only exact-stream message-read and reply access
   for retrieval. This local profile still trusts the operator administering files.
4. Apply the reviewed consecutive enabling revision using `offline-control` and the
   existing schema-owner authority. Inspect both scope and implementation switches,
   the approved exact profile and the configured owner read authority. Enabling one
   switch does not override another disable. The command below independently checks
   the same current publication fences at commit.
5. In the existing configured Research environment, submit the private envelope file:

   ```sh
   edgeagent-research offline-once < retained-envelope.json > recovered-outcome.json
   ```

   Closed stdin is required. The command bounds input and processing. Require a
   successful exit and inspect the canonical outcome, including an explicit
   abstention if owner evidence is no longer eligible. A disabled switch, foreign
   scope or unavailable dependency is a refusal, with no new successful completion.
6. Preserve the canonical receipt and audited enable revision in the private ticket.
   After a lost response, retry the exact same bytes. An existing receipt recovers
   before a new owner read. Drain committed publication through the existing bounded
   `offline-relay` configuration and verify the projected outcome.
7. Resume intake after verification. The original stream entry and exhausted durable
   remain retained. Manual processing does not ACK, delete, reset or replenish that
   durable. If it is redelivered by a separately authorized future broker action,
   normal receipt-first processing deduplicates it. Close the operational incident
   using the receipt, not a claim that the broker's pending count was cleared.

A scope or profile change during recovery can refuse processing. Retain the same
input and diagnose before retrying. Never change IDs, republish to bypass limits,
erase control history or substitute newer market evidence. Restoring an absent
message requires a separately approved retained-evidence recovery procedure.

## Verification

Run `cargo test --locked -p edgeagent-contracts --test research_controls` and
`cargo xtask verify --check research-persistence --check research-worker-postgres --check research-replay-postgres --python python3`
with an isolated loopback PostgreSQL URL. Add an isolated NATS URL for
`cargo xtask verify --check research-worker-broker --check research-processes --python python3`.
Run `make repository architecture PYTHON=python3`, `make verify PYTHON=python3` and the
canonical SQLx metadata gate with its documented tooling. Online macro compilation
is separate evidence and cannot claim the canonical Compose/CLI gate passed.
