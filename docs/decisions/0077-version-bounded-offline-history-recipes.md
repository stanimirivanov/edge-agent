# ADR-0077: Version bounded offline history recipes

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Add explicit finite six-session invented recipes for M04 research prerequisites.
- Preserve the original one-bar recipe and its exact admission identity.
- Reject missing history and unsupported action annotations through existing quality policy.

## Context

Momentum needs more than the original one-bar offline seed. Existing admission
configuration binds exact implementation bytes, so editing that code without a
compatibility provision would reject pending legacy work.

## Decision

Retain the original source as an immutable fixture and use its exact bytes for
legacy implementation/configuration binding. The generalized producer must match
its original admission record in a regression test. New explicit recipe names bind
current producer bytes, retained observations and selected authority into admission.
Restore by exact record equality; never substitute a default recipe. Completed
receipts continue to recover before reconstructing source work.

History has six invented UTC daily sessions, raw prices and admission-time
availability. Missing-history and unsupported-action variants are negative fixtures.
Existing quality policy refuses them; no gap filling, adjustment or rights bypass
is introduced. The source stays network-free and has no live selection path.

## Alternatives considered

Changing the default to history would break the one-bar profile. Duplicating the
production constructor would create diverging policy paths. An unbounded fixture
DSL or new provider abstraction is unnecessary for this research prerequisite.

## Consequences

Synthetic weekend sessions must not be advertised as actual exchange sessions.
Six-session historical readiness has explicit finite age/skew limits. Domain
momentum policy is not implemented. M03 continues to own real source quality.

## Compatibility and migration

No schema migration or old-record rewrite is required. Preserve the frozen source
fixture; future recipe changes use a reviewed new identity. Existing CLI arguments
retain their original default. Unknown recipe names fail before infrastructure.

## Security and operations

No credentials, paid resources, cluster operations or vendor factories are added.
Rollback to the old worker stops unfinished history jobs by configuration mismatch;
retain their records and objects until a compatible worker resumes them. Legacy
jobs and already committed receipts remain recoverable.

## Validation

Run the default gate and `offline-worker-contracts`. Tests compare original and
current legacy admission, repeat source bytes after restore, reject changed
configuration and exercise successful and negative quality preparation.
