# ADR-0086: Persist and serve scoped Research projections

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- The projector owns its request anchor, pure checkpoint, inbox and applied-delivery marker.
- Commit successful projection and consumption evidence together; ACK afterward.
- Query absence means not observed; unresolved owner conflicts remain visible as gaps.

## Context

The pure Research reducer preserves canonical outcomes but does not establish durable
consumption or a query interface. Portable delivery metadata supplies opaque keys,
not global offsets. Existing terminal owner events have no revision chain, and a
view must not invent sequencing or present lag as authoritative rejection.

## Decision

Add a projector-private schema and explicit local tracking, one-record projection,
finite JetStream consumption and loopback internal GET modes. Validate exact Research
terminal publication identity/source/route using existing contracts, initialize from
its authenticated request when necessary and apply the pure reducer. Persist the
checkpoint, inbox receipt and applied-delivery key in one READ COMMITTED transaction.
Duplicates acknowledge original effects without changing checkpoint/progress.

A conflicting authenticated owner record preserves the prior checkpoint/marker,
retains one bounded unresolved envelope/key and returns Gap. No new inbox receipt
or successful progress is committed for it. Envelope identity conflicts roll back
the inbox transaction before committing their separate diagnostic gap. Gapped work
remains unacknowledged; operator-controlled rebuild/backfill is required. Malformed,
foreign and misrouted input is quarantined durably before broker termination.

Expose scoped internal status and exact outcome endpoints. Metadata includes local
projection revision and the opaque applied key, never a global watermark. Pending,
artifact, abstained, gap, not-observed and unavailable remain distinct. Restore and
revalidate checkpoints and their original inbox publication proof before serving
terminal bytes. A gap blocks the outcome endpoint instead of serving an older view
as current. Only explicit trusted local tracking creates pending rows before outcomes.

## Alternatives considered

Peer owner-table reads violate ownership. ACK-before-commit can lose a projection;
separate checkpoint/inbox commits can duplicate or skip effects. Parsing an opaque
transport key into a global cursor invents unsupported semantics. Silently replacing
conflicting terminal data would erase an owner inconsistency. Public Gateway routing
and lifecycle routes belong to later slices.

## Consequences

This is a rebuildable read model, not owner state or current evidence entitlement.
Queries can be behind the owner; their snapshot/revision cannot certify global
currency. Known conflicts remain visible until explicit repair. Retain inbox proofs
with checkpoints; deleting either independently makes terminal reads unavailable.
Checkpoint checksums detect corruption, not authorized rollback. Storage trust and
broker identity/provisioning remain external local-operator obligations.

## Compatibility and migration

Apply the additive projector migration and all inbox migrations in a new private
schema before activation. Startup performs no DDL or topology creation. Existing
account/Research checkpoints, owner records and producer routes do not change.
Roll back by stopping projector intake/query serving. Rebuild only disposable
projector tables/inbox with coordinated consumer replay; never erase owner state
or retain progress while discarding its checkpoint. Lifecycle support remains PR13.

## Security and operations

Listener and database/broker URLs require loopback; URL query/fragment overrides
are refused. One fixed configured operator scope governs intake and query paths.
Reject browser Origin, nonlocal Host and HTTP writes. No public authentication,
provider/model/evidence object access or live execution is introduced. Use bounded
pools, payload/checkpoint sizes, transactions and requests. Errors preserve typed
causes while normal HTTP/CLI diagnostics omit SQL, payload and credentials.

## Validation

Protect default HTTP/configuration/security tests and real PostgreSQL proofs for
pending/terminal reads, exact bytes, duplicate process restart, statement/deferred-
commit rollback, killed-worker rollback, conflicts/gaps and corrupt/unavailable
snapshots. Real JetStream tests cover both outcome subjects, ACK after commit,
duplicate restart, misroute quarantine and unresolved gap without progress advance.
Extend canonical SQLx assembly and keep its unavailable prerequisites explicit.
