# ADR-0102: Fence fresh Research with audited kill switches

- Status: Accepted
- Date: 2026-10-31
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Caller-scope and implementation switches compose conservatively for every fresh completion.
- Record revision-checked operator actions with authenticated actor and transaction time.
- Fence changes against completion and retain the checked revisions atomically.

## Context

PR12 serializes approval/retirement with completion. PR13 needs an independently mutable
operational stop without rewriting strategy configuration or historical artifacts. The user
confirmed caller-scope and implementation-digest targets, schema-owner administration,
no-recorded-switch means enabled, and non-retroactive behavior.

## Decision

Accept only bounded canonical research.control.v1 commands with scope, target, consecutive
positive revision, enabled state and a bounded reason code. There is no client actor/time.
Use the existing authenticated schema-owner authority. Append immutable history recording
session_user and transaction_timestamp; exact latest retries require the same actor/content.
Stale, skipped or conflicting revisions reject. An explicit enable is audited too and never
approves a strategy or overrides another disabled switch.

Control administration holds the same exclusive scope advisory lock as approval changes.
Governed pre-read recovery and fresh completion check the latest action for both targets
under the shared scope lock, held through transaction commit. Either disabled target blocks
all new completions, including abstentions. Missing history means the explicit enabled
default; missing tables, permissions or unavailable storage do not mean enabled.

Retain both checked control revisions with the immutable decision. Zero identifies a checked
no-history default; null identifies pre-control historical absence. A deferred database
constraint requires a control snapshot for every post-migration completion, blocking older
writers. Historical recovery verifies referenced enabling actions and preserves existing
bytes even after later disables. It does not check current permission or invent provenance.

Expose only the local offline-control operator command. Disabled is a distinct safe
application error, not malformed caller intent. Broker deliveries are NAKed with bounded
delay, without successful inbox/progress or poison classification, including at the delivery
ceiling. Already committed outbox events, lifecycle actions and receipt/read-only replay
remain available. Enabling does not automatically reset a broker delivery budget.

## Alternatives considered

An environment Boolean cannot provide durable actor/reason/history or fence concurrent
workers. A check only before computation races a later disable. Treating disabled input as
poison discards valid backlog. Dropping committed outbox events violates atomic publication.
A global cross-tenant switch or new broker command/control framework exceeds this scope.

## Consequences

A disable that commits first prevents new completion effects; an admitted completion holding
the shared lock commits before the disable can commit. The retained revisions explain which
publication-time controls were checked, independently of historical decision cutoff time.
The unchanged decision event does not claim to expose these private owner audit bindings.
Scope targets cover current/future implementations in that caller scope; implementation
switches cover all configurations of that exact digest, not future implementation digests.

## Compatibility and migration

Stop Research intake and apply migration 0005 after 0001–0004. No initialization action is
fabricated and no old row is rewritten as having checked controls. Runtime needs controls
SELECT plus existing decision INSERT/SELECT; administration needs separately controlled
owner credentials. No startup migration, automatic enable or public control endpoint exists.
No Research v1 or decision-event schema changes, dependency addition or new broker subject.

Rollback stops intake and preserves history, snapshots and constraints for a compatible
binary. Old binaries cannot create fresh unfenced completions after migration. Do not drop
control state or constraints to restore availability. Disabling does not withdraw already
published artifacts; use the existing explicit lifecycle boundary for that separate action.

## Security and operations

Canonical hashes and audit rows do not create operator authority. The local shared schema
owner model remains unchanged; it does not claim multi-user separation of duties. Runtime
roles must not receive control INSERT/UPDATE/DELETE or DDL/TRUNCATE/trigger-disable grants.
Models, strategy output and business messages cannot issue control commands. Account-risk
controls, configured automatic invalidation and broader rollout remain separate work.

## Validation

Strict contract proofs reject malformed/unknown fields, spoofed actor/time, stale shapes,
duplicate keys and oversized input. Real PostgreSQL proofs cover action retries/conflicts,
AND composition, actor authority, immutability, both observed race orders, statement/commit
rollback, pinned-history refusal, legacy absence and old-writer rejection. CLI and broker
proofs cover recovery, scope refusal, committed relay and non-poison blocked redelivery.
Run default verification and canonical SQLx metadata qualification when prerequisites exist.
