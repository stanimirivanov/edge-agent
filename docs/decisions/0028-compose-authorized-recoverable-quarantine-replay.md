# ADR-0028: Compose authorized recoverable quarantine replay

- Status: Accepted
- Date: 2026-10-18
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- A restricted PostgreSQL-authenticated operator authorizes replay in its own
  schema; request text cannot choose the audit actor.
- Inbound authorization and immutable dispatch intent share one commit. A
  different dispatcher publishes exact retained bytes after commit, then records
  a fenced durable receipt. Outbound replay composes the existing release API.
- Replay-request transport identity differs from immutable domain identity.
  Replay uses the original route, can reach other subscribers, and never repairs
  malformed evidence or promises exactly-once delivery.
- This is conformance control-plane tooling, not production IAM or a new generic
  worker/workflow framework.

## Context

The inbox adapter appends replay authorization and returns retained bytes, but
does not durably schedule publication. An authorizer can therefore exit after
commit but before publishing. The outbound adapter already atomically appends
audit evidence and releases quarantine while preserving its claim generation.
M02 requires authenticated composition and restart-safe inbound dispatch, not
changes to those portable/application boundaries.

## Decision

Add finite `replay-authorize` and `replay-dispatch` roles to the tooling worker.
Authorization derives actor identity from PostgreSQL `session_user` and checks
that both the login suffix and current schema match the configured owned schema.
An operator login ends in `_o`; a distinct dispatcher ends in `_d`. Neither an
input actor field, a role flag nor `SET ROLE/current_user` grants authority.
Component configuration belongs to the trusted composition root, not the request.

For inbound replay, the operator calls the existing audit API, validates exact
retained bytes and the configured subscriber/original route, and inserts a
dispatch snapshot in that same transaction. Re-entry with the same request is
idempotent; a different target, reason or actor conflicts. Invalid, unsupported,
misrouted or foreign-subscriber evidence rolls back the audit as well as intent.
Quarantine remains intact. The dispatcher also compares its snapshot against
retained audit/quarantine evidence and revalidates the route before publication.

Dispatch claims one row with `SKIP LOCKED`, commits a ten-second lease and a
monotonically increasing generation, and only then publishes. Each acquired claim
consumes one of three attempts, including interrupted or unknown attempts. It
publishes exact original bytes with `Nats-Msg-Id` derived from owned schema and
replay-request ID. The body, CloudEvents source/ID and all causal metadata remain
unchanged. A new intentional replay requires a new request ID; ordinary producer
message deduplication cannot suppress that new request. Broker deduplication is
bounded, so consumer inbox and semantic guards remain authoritative.

A confirmed broker receipt must name the expected stream and a positive sequence.
Completion compares current owner, generation, unexpired lease and unpublished
state, then atomically records publication and an append-only receipt. Failure,
cancellation or lost results retain intent; no destructor publishes or compensates.
After expiry a new process can recover within budget. Exhaustion requires operator
inspection, not automatic budget reset. No claim of exactly-once publication is
made, including after the broker deduplication window expires.

Outbound authorization validates the retained supported route and calls
`PostgresOutbox::replay_quarantined` inside its own commit/rollback boundary.
Only the production relay publishes afterward. Audit preserves the previous
failure/attempt evidence; reset of attempts never resets claim generation.

## Alternatives considered

- Publish directly from the authorizer: loses intent after commit/process exit
  and confuses unknown external effects with durable success.
- Enqueue replay bytes as an ordinary outbox message: the original immutable
  identity already exists, and forging denormalized identity violates relay
  validation. Dedicated replay-request identity belongs to dispatch, not the body.
- Rewrite the domain ID or wrap/repair the payload: breaks immutable evidence,
  causation and existing inbox semantics.
- Give ordinary workers replay authority or trust `requested_by`: permits
  self-authorization and loses the authenticated actor boundary.
- Target a private subscriber subject: changes routing/fan-out and invents an
  exclusive-delivery guarantee unsupported by the existing event contracts.

## Consequences

Tooling adds service-owned dispatch/receipt tables and checked SQLx descriptions.
No adapter, port, published event schema, product service or dependency changes.
The separate broker replay principal may publish only registry-derived incoming
subjects and subscribe only to its private reply inbox. It has no intake, ACK or
topology administration rights. That inbox is a sibling of the normal runtime
prefix, not a child accidentally covered by its wildcard subscription.

Operators cannot mutate retained payloads, dispatch leases or receipts. Dispatchers
cannot authorize replay or change intent bytes. Ordinary runtimes receive neither
capability. Database administrators and trusted composition-root credential holders
remain privileged; this is not protection against a compromised database owner.
Column grants and SQLSTATE/real broker permission-event proofs establish these
capability limits, rather than timeouts or assertions about configuration alone.

## Compatibility and migration

Create fresh disposable conformance and metadata schemas; never apply these tables
to product databases. Historical adapter migrations and ADRs remain unchanged.
Do not mix old/new tooling or reconstruct missing dispatch history from current
domain state. Published v1 bytes and baseline schemas remain unchanged. Rollback
uses fresh old-version fixtures, not deletion of accepted audit, quarantine or
inbox evidence. Outbound release preserves existing retry/fencing compatibility.

## Security and operations

**[Prerequisite from M10]** PostgreSQL identities, scoped broker principals,
bounded requests, deadlines and evidence inspection are the minimum proof control
plane. Public local-only fixture passwords are not production credentials or IAM.
Production authentication, secret delivery, UI/API authorization, dual control and
fleet operations remain M10. Operator requests are bounded to 4 KiB, contain no
actor field and never select an arbitrary publish subject. Worker failure output
omits payloads, credentials and source chains.

Original event-route replay reaches every eligible subscriber. Previously
processed subscribers may observe a duplicate and must retain normal idempotency.
An audit target identifies authorization evidence, not exclusive delivery or proof
of domain completion. The [runbook](../development/quarantine-replay.md) distinguishes
authorized, leased, confirmed and unresolved/exhausted states and safe recovery.
No model calls, market data, live execution or cloud qualification are introduced.

## Validation

The protected process inventory proves real unauthorized principals and column
denials, forged actor rejection, atomic rollback, request conflicts, unsupported
evidence rejection, independent-process restart, killed-worker result loss,
same-request duplicate receipts, fresh-request inbox suppression, original-route
fan-out, broker permission denial, and outbound audit/budget/fence preservation.
Pure tests protect scope derivation and redaction. Census mutation tests reject
missing, renamed, extra or duplicate proofs; the independent floor increases with
the four mandatory cases. Default compilation/discovery is not runtime evidence.
Result loss occurs after a real persisted receipt, not by a wire ACK proxy;
wire-ACK-loss and full crash/broker qualification remain M02 slice 7.
