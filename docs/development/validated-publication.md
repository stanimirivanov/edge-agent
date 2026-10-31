# Validated Research publication

## TL;DR

- Require an explicitly approved complete execution configuration for fresh publication.
- Commit configuration, validated decision, inbox and both publication intents together.
- Preserve legacy receipts and upgrade readers before producing companion events.

M05-PR12 closes the staged publication boundary in
[ADR-0101](../decisions/0101-commit-validated-research-decisions-with-publication.md).
A fresh completion requires a complete canonical configuration, authorized owner
policy evidence, independent reconciliation and current exact profile approval.
Existing Research v1 outcome bytes and historical receipts retain their meaning.

## Configuration and approval

Set `EDGEAGENT_RESEARCH_EXECUTION_CONFIGURATION` to a reviewed operator-controlled
file containing canonical `research.execution.v1`. Its data includes `bundle` (the
unchanged strict [PR2 configuration](strategy-policy.md)), explicit `event_revision`,
`context_max_age_ms` and sorted unique `allowed_regimes` (falling, flat, rising).
There are no inferred defaults. The bounded file is at most 16,384 bytes.
The [invented test configuration](../../crates/contracts/fixtures/strategy/execution-v1.json)
is a format example, not production threshold advice.

Select the compiled revisions shown in that fixture. The invalidation revision
`owner-lifecycle-v1` means current explicit owner lifecycle only; PR14 supplies
configured automatic invalidation. Benchmark identifies a retained declaration;
this slice produces no benchmark metrics or scenario/risk illustrations.

The exact execution-file digest is `EDGEAGENT_RESEARCH_CONFIGURATION` and the
request configuration. Existing environment implementation, maximum age and TTL
must match the file; lookback comes from its strategy declaration. Unknown formulas,
partial bundles or mismatches refuse. Keep operator configuration outside the
MarketData directory that `offline-init` requires to start empty.

Create the exact [approval Profile](strategy-governance.md) with that digest and
matching scope, implementation, lookback, evidence age, expiry, formula and renderer.
Use existing install/approve commands explicitly. Approval of an old M04 digest
never transfers to a new execution file. No startup path approves anything.

## Storage and publication

Stop all Research intake before migration, then apply [0004](../../crates/research-postgres/migrations/0004_research_decisions.sql)
after Research migrations 0001–0003 and all existing inbox/outbox migrations.
Grant runtime SELECT/INSERT on `edgeagent_research_decisions`; preserve existing
grants and deny DDL/TRUNCATE/trigger-disable rights. No peer reads this owner table.
Startup still does not migrate. Old rows explicitly lack provenance; every fresh
insert must acquire its companion before deferred commit, including old binaries.

`Selection` resolves every selected control, constructs its own policy, checks exact
expiry capped by rights, and independently reconciles the proposal. Only sealed
`ValidatedExecution` can create `PreparedCompletion::from_execution`. Lower-level
legacy preparation APIs remain useful for compatibility/recovery but cannot commit
a fresh result. The old worker function is receipt recovery only.

`offline-once`, `offline-governed-once` and `offline-worker` require the new selection
for fresh execution. One-shot historical recovery runs before loading configuration
or the owner. Governed fresh recovery checks approval before owner I/O. Commit
checks current approval again under the scope advisory/profile locks that fence
retirement. A successful transaction retains configuration, decision, terminal row,
inbox and two exact outbox events together. Failure/cancellation rolls back all.
Owner-denied/corrupt/insufficient inputs remain explicit abstentions; unavailable
reads retry without completion. Direct legacy evaluation lacks evidence permission
and drains to MissingEntitlement under the same complete configuration and approval.

The new event is `com.edgeagent.research.decision-recorded.v1`, schema
`urn:edgeagent:schema:research:decision:v1`, with the same scoped partition. Its ID
and idempotency key are the companion digest; causation is the terminal digest.
Terminal v1 publication is unchanged and enqueued first. Do not assume broker delivery
order. Retain both outbox proofs and immutable approval history for receipt integrity.
No broker I/O occurs inside the completion transaction.

## Projection and queries

Provision the decision subject alongside published/abstained/lifecycle subjects.
The projector durable consumer now requires all four explicit filters. Missing
outcomes return Gap without successful inbox/progress; backfill the terminal and
retry the companion. Duplicate facts remain idempotent. Read queries restore the
checkpoint and revalidate both terminal and provenance inbox proofs.

Projector metadata adds optional `decision` (digest); local revision counts the
additional observed fact. Upgrade Gateway's strict metadata reader before producing
it. `/internal/v1/research-requests/{scope}/{id}/decision` returns exact canonical
historical bytes: 200 when observed, 404 not_observed for legacy/pending/unknown,
409 for gap, 403 foreign scope/Origin/Host and 503 corrupt/unavailable. A closed
artifact can still have historical provenance; none of these observations asserts
current publication or account permission. Public decision-body Gateway routing is
not added; existing metadata/outcome forwarding retains its scope and exact bytes.

## Verification and rollback

Run `make repository architecture PYTHON=python3` and `make verify PYTHON=python3`.
With an isolated loopback PostgreSQL URL, run `cargo xtask verify --check research-persistence --check research-worker-postgres --check projector-postgres --python python3`.
With an isolated NATS URL too, run `cargo xtask verify --check research-worker-broker --check projector-broker --check research-processes --python python3`.
Run `cargo xtask verify --check sqlx-metadata --python python3` with its documented
canonical Compose/CLI environment. Equivalent online macro generation does not
claim that canonical gate passed. Regenerate the local decision schema using
`cargo run --locked -p edgeagent-contracts --features schema --example research_decision_schema`.

Stop intake, relay and serving before downgrade; preserve owner data and approval
history for a compatible binary. Rebuild disposable views from retained authenticated
facts only. Never strip companions or disable the database requirement to downgrade.
Kill switches, automatic invalidation and full policy-decision replay remain PR13,
PR14 and PR17 respectively; the current replay verifier does not claim to verify a
policy-denied result by recomputing a bare strategy artifact.
