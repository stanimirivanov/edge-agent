# Configured artifact invalidation

## TL;DR

- Select retained-eligibility-expiry-v1 in strategy.invalidation_revision and approve the new configuration digest.
- Run a bounded explicit-time owner check against retained requests; no scheduler or acquisition runs.
- Lifecycle and outbox append together; original artifact and decision bytes never change.

## Operator workflow

Use the existing prepared Research schema through migration 0005 and authenticated
schema-owner credentials. Keep worker credentials separate. Install and approve the
new exact execution profile before publishing artifacts with the new revision.
Existing owner-lifecycle-v1 artifacts remain explicit-only; checks refuse them.

Run `edgeagent-research offline-invalidation-check` with the existing loopback
EDGEAGENT_POSTGRES_URL, EDGEAGENT_RESEARCH_SCHEMA and EDGEAGENT_RESEARCH_SCOPE.
Closed stdin contains one canonical command of at most 16384 bytes. The strict
[Command](../../crates/contracts/src/research_invalidation.rs) has kind
research.invalidation-check.v1 and data fields original (the exact complete
ResearchRequest), effective_at_ms, and reassessment (another complete request or
explicit null). Keys are recursively sorted with one final newline. No evidence,
actor, mutable policy, threshold or configuration file is accepted in the command.
Use Command::bytes to encode it; malformed, foreign and unknown fields refuse.

At effective_at_ms greater than or equal to the retained expiry, append Expired.
Before expiry, require the named reassessment to have already committed through
normal validated publication. It must be strictly later than the original, no later
than effective time, and match scope, implementation, configuration, lookback and
universe exactly. Its evaluated eligibility must exclude at least one originally
ranked instrument to append Invalidated. Rank changes alone do not invalidate.
A complete assessment retaining every ranked instrument succeeds with empty stdout
and no state change. Missing, unavailable, corrupt or NotEvaluated assessments refuse
without writes. This is historical eligibility evidence, not a current-market claim.

Closure returns the exact lifecycle event envelope on stdout. An already closed
artifact returns its latest intact lifecycle event; no new revision, revival or
replacement closure is created. Retry after response loss or restart. The original
receipt remains recoverable. The invalidation event causation identifies the exact
retained decision event; retain it and its configuration, approval, control history
and publication proofs with the original. A hash or caller request alone is not proof.

## Boundaries and rollback

Checks share existing lifecycle serialization with manual correction, withdrawal
and expiry. Validation plus lifecycle/outbox insertion commit atomically. There is
no peer SQL, fresh evidence I/O, host clock, new broker subject or migration.
Existing relay, projector and Gateway closure/gap behavior applies. Switch disable
or profile retirement does not stop historical lifecycle checks. Incomplete evidence
does not silently revoke or confirm continued eligibility.

This is an explicit invoked check, not continuous surveillance. Operators arrange
check cadence and independently validated reassessments. Stop checks and fresh
publication using the revision before binary rollback. Keep authoritative history
and pending events. Older Research binaries reject the new configuration revision;
do not rewrite retained configuration or remove lifecycle history to downgrade.
See [ADR-0103](../decisions/0103-apply-configured-retained-decision-invalidation.md).

## Verification

Run `cargo test --locked -p edgeagent-contracts --test research_invalidation`,
`cargo xtask verify --check research-rendering --check research-lifecycle --python python3`
and `make verify PYTHON=python3`. With an isolated loopback PostgreSQL URL, run
`cargo xtask verify --check research-persistence --check research-worker-postgres --python python3`.
These cover configured policy, real commit failures/concurrency, schema-owner
refusal and actual CLI restart recovery. Existing checked SQL statements and cached
metadata are reused; no metadata or migration generation is needed for this slice.
