# ADR-0103: Apply configured retained-decision invalidation

- Status: Accepted
- Date: 2026-10-31
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Opt into retained-eligibility-expiry-v1 through immutable configuration and new approval.
- Check explicit time and intact retained decisions; never accept caller market facts.
- Append existing lifecycle/outbox events atomically without changing artifact bytes.

## Context

PR12 resolves owner-lifecycle-v1, which only supports explicit owner transitions.
PR14 needs reproducible configured invalidation without inventing surveillance data
or allowing strategy output to establish policy facts. The user confirmed expiry
and retained validated reassessment semantics, preserving legacy behavior.

## Decision

Accept a bounded canonical research.invalidation-check.v1 local operator command.
It identifies the complete original request, an explicit effective time and optional
complete reassessment request. Authenticate the existing PostgreSQL schema owner.
Resolve configuration from the intact original decision companion. Reject legacy
explicit-only configurations and missing/corrupt retention without mutation.

At or after the retained expiry cutoff append Expired; expiry takes precedence and
needs no reassessment. Before expiry require a strictly later retained validated
Evaluated decision with identical scope, implementation, configuration, lookback
and universe, at or before effective time. Append Invalidated if any original
ranked instrument is excluded. Full exclusion qualifies; rank/score changes alone
do not. NotEvaluated abstentions, including missing or unsafe critical evidence,
refuse the check. Complete eligibility with no exclusion returns unchanged.

Use the existing scope lifecycle advisory lock through commit, including checks
and exact outbox enqueue. Concurrent checks or explicit commands serialize. An
already closed artifact returns its latest intact event without another transition;
checks never revive artifacts or replace existing closure facts. The invalidation
publication causation identifies the exact retained decision event. Expiry retains
the existing owner-event causation and original configuration/cutoff provenance.

## Alternatives considered

Caller-supplied prices or eligibility sets would bypass owner validation. Reusing a
legacy revision for new behavior would silently reinterpret immutable configuration.
A new scheduler, generic rule engine or data acquisition loop is unnecessary for a
bounded check. Treating unavailable evidence as a negative assessment would invent
policy conclusions. Host time would undermine deterministic replay of the check.

## Consequences

Operators must supply and retain separately validated reassessments and explicitly
invoke checks. This slice does not claim continuous surveillance or automatic expiry
at wall-clock cutoff. Kill switches and retirement remain non-retroactive: they do
not prevent checks of historical retained evidence or delivery of committed events.
Existing projection, query closure, gap/retry and immutable artifact semantics apply.

## Compatibility and migration

No schema, broker subject, dependency or migration changes. A new revision changes
the configuration digest and requires installation/approval before fresh publication.
Historical owner-lifecycle-v1 configurations retain their exact behavior. To roll
back, stop checks and new publication using the revision; preserve all events and
use compatible readers for retained new-revision decision configurations. Older
Research binaries cannot recover those new configurations. Do not rewrite them.

## Security and operations

The command grants no authority, supplies no evidence bytes and reads no peer tables.
Authenticated schema ownership is the local operator boundary, not multi-user
separation of duties. Runtime roles should not receive owner credentials. Explicit
effective time is an operator assertion, not a claim of present market freshness.
No public mutation endpoint, broker control route, model or live execution is added.

## Validation

Contract and pure tests cover canonical bounds, binding, exclusions, incomplete
assessment refusal and exact expiry boundaries. Isolated PostgreSQL proofs exercise
retention integrity, default compatibility, unchanged eligibility, retries/restarts,
concurrency, statement/deferred-commit rollback and nonowner refusal. A real CLI
proof expires an opted-in artifact and recovers the exact event after restart
without configuration files or an evidence adapter. Run the mandatory default gate.
