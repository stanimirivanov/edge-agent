# Revisioned account audit replay

## TL;DR

- ExecutionSimulator retains genesis and every committed account revision.
- Owner exports, not peer SQL reads, feed disposable account/order/risk views.
- Duplicates are no-ops; gaps require backfill; conflicts fail without mutation.
- Checkpoints retain duplicate receipts; their checksum is not authentication.
- Synthetic finite-run qualification only; no public query or broker activation.

Read [ADR-0041](../decisions/0041-retain-owner-authored-account-audit-revisions.md)
before changing capture, replay or export compatibility.

## Capture and ownership

The `contracts::audit` export DTO is distinct from the existing outcome messages.
Those facts do not contain genesis funding, complete positions, holds or risk
continuations; attempting to reconstruct those values would invent history.
`simulation-postgres` captures a full checked revision after authoritative effects
and before the inbox transaction commits. Initial revision zero is captured under
the account-root lock on the first successful canonical mutation, after bootstrap
is complete. Both records roll back with a failed first command. Execution
bootstrap cannot edit inventory/book costs once history has started.

The append-only runtime log includes frozen run/configuration and optional
execution/settlement plan bindings; account/input revision; exact cash and
cumulative balanced postings; journal-replayed order status and quantities;
remaining reservations; inventory and explicit optional book costs; risk-policy
digest/revision/original breach/checkpoint; original valuation timestamps,
artifact digests and conservative gross/loss carry. Carry is not a new market
mark. Projection code never invokes admission or risk decisions.

`export_audit` is an ExecutionSimulator-owned adapter API bound to a verified
principal and original context. A service composition must authenticate and
authorize before calling it. It is not an endpoint or an invitation to grant
AuditProjector access to private tables. Public transport, scoped export identity,
durable projector deployment and browser APIs remain unactivated. The read model
consumes only checked exported records, not the owner adapter or schema.

## Replay and recovery

Start `AccountProjection` with an authenticated revision-zero frame and explicit
capacity. Apply only contiguous revisions bound to the same account/run/config/
plans/policy. Each predecessor names the SHA-256 of canonical checked content.
Exact duplicates remain no-ops even after capacity exhaustion. Different content
at a retained revision, a changed predecessor, foreign scope or financial drift
leaves the entire view and receipt list unchanged. Out-of-order delivery returns
`Gap`: fetch the missing prefix through the owner API, then retry the record.
There is no unbounded gap buffer or speculative latest-state replacement.

Views report their revision and lag against an owner-provided high-water mark,
not an unsupported wall-clock freshness claim. Owner pages bound the exclusive
cursor and immutable results to the observed high-water. An empty page before
that mark is missing history, not completion. Genesis plus all revisions rebuilds
the same view as checkpoint/resume. Snapshots replace read-model state; they do
not reapply financial effects to the simulator.

Persist checkpoint bytes and consumer progress together in projector-owned
storage. `checkpoint` retains state and all duplicate fingerprints; `restore`
checks version, size, integrity, receipt census, latest digest and balances.
Restore requires the authenticated original genesis and rejects a changed account,
run, configuration, plan, policy or funding baseline before exposing a view.
Authenticate and fence checkpoint storage separately. The checksum cannot detect an
authorized rollback or maliciously rewritten checkpoint. Replay never confers
financial authority and cannot reset sticky breaches or refresh valuations.

## Bounds and compatibility

Records are versioned `edgeagent.account-audit.v1` owner exports, not new registry
CloudEvents. Existing admission/execution/settlement v1 schemas remain unchanged.
Decode rejects unknown versions/fields before use and bounds input at 1 MiB.
At most 4096 orders/positions and 32 valuations fit in one state; byte exhaustion
fails closed rather than truncating history. Pages contain 1..=8 records (up to
8 MiB); checkpoints fit within 3 MiB. Explicit projection capacity is 1..=8193
revisions including genesis, covering the finite base schedule and postlude.

Capture replays existing bound order journals one at a time and aggregates retained
financial receipts. This favors correctness for bounded synthetic runs, not a
claim of hot-account throughput. Existing transaction budgets still apply;
production load/incremental capture qualification belongs to M10. No new dependency
or private journal representation leaks into the export contract.

Migration 0004 is additive; apply it before rolling workers and grant runtime
SELECT/INSERT only on `simulation_account_audit`. Do not grant UPDATE/DELETE.
Existing accounts with nonzero revisions and no retained genesis cannot enroll
by copying their current balance: new mutations fail `audit_history_missing`.
Run them to completion with the old version or start a separately identified,
explicitly funded new run. No financial backfill or reconstruction is guessed.
Rollback pauses ingress and preserves all immutable records. An older writer
must not resume a logged run because it would omit later revisions.

## Verification

`cargo xtask verify` includes protected `account-audit-contracts` discovery and
six named default replay/contract proofs. `cargo xtask verify --check
account-admission-processes` adds real restricted-worker rebuild, rollback,
crash/lost-result and risk-view reconciliation. `cargo xtask verify --check
sqlx-metadata` verifies checked queries against migration 0004. These do not
claim public deployment, NATS audit-feed publication, high availability, database
power-loss recovery, provider fidelity or completion of all M08 work.
