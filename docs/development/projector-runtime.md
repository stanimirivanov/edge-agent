# Persistent Research projector and internal queries

## TL;DR

- The projector owns its schema, checkpoint/inbox and opaque applied-delivery marker.
- ACK only after commit; unresolved owner conflicts preserve the prior checkpoint as a gap.
- Internal GETs enforce a fixed trusted scope; absence means not observed, not rejection.
- Prepare schema/topology explicitly and rebuild only disposable projector state.

## Configuration and activation

The binary keeps its descriptor commands and adds `offline-track`, `offline-once`,
`offline-worker` and `serve`. All modes require loopback `EDGEAGENT_POSTGRES_URL`
(no query/fragment routing overrides), `EDGEAGENT_PROJECTOR_SCHEMA` (a `projector_`
prefix, lowercase ASCII/digits/underscore, at most 63 bytes) and
`EDGEAGENT_PROJECTOR_SCOPE` (1–64 ASCII letters/digits/period/underscore/hyphen).

Apply the [projector migration](../../services/audit-projector/migrations/0001_research_projection.sql)
and all inbox migrations to the same independent private schema before activation.
Startup never upgrades or creates it. Use a runtime role with required projection/
inbox/quarantine permissions, no DDL/TRUNCATE/trigger-disable privileges and no peer
owner-table access. All data here is a disposable projection, not Research ownership.

`offline-track` reads a bounded canonical Research Request record from closed stdin
and records an independently trusted anchor as pending. This is an operator-only
tracking assertion, not Gateway/business acceptance. `offline-once` reads one bounded
canonical Research owner envelope and projects it; its manual delivery key is not
a broker offset. Both inputs are capped at 262144 bytes before decoding.

`offline-worker` also requires loopback `EDGEAGENT_NATS_URL`, explicit
`EDGEAGENT_PROJECTOR_STREAM`, `EDGEAGENT_PROJECTOR_CONSUMER` and
`EDGEAGENT_PROJECTOR_MAX_MESSAGES` (1–16). Provision a durable pull consumer with both
Research PUBLISHED and ABSTAINED subjects, no broad single filter, explicit ACK,
at least 60-second ACK wait, three maximum deliveries and 1–32 pending ACKs. The
consumer validates actual subject against payload route before projection. SIGINT/
SIGTERM stop intake; each operation is bounded. No topology or deployment is created.

## Atomic projection and unresolved input

The [store](../../services/audit-projector/src/store.rs) validates existing Research
terminal publication identity, source, schema, scoped subject/partition, request
binding and canonical outcome bytes. Trusted owner data can supply the initial
request independently of checkpoint storage. A scoped row lock serializes competing
writers; READ COMMITTED, five-second lock and fifteen-second statement timeouts apply.
Checkpoint, inbox and successful applied-delivery key commit together. Duplicate
inbox deliveries commit no new domain/progress writes, including process/response
loss recovery. The key is opaque per-delivery evidence, not global consumer position
or an inferred stream high-water mark. Local view revision remains zero/one.

Conflicting authenticated owner data or source/ID envelope reuse commits one bounded
unresolved envelope/key while preserving the checkpoint/applied marker. The first
conflict is retained; further deliveries do not overwrite it or clear the gap.
An inbox identity conflict rolls back that transaction before a separate diagnostic
gap transaction. No new successful inbox receipt/progress is recorded for unresolved
work. It is NAKed after fifteen seconds, not ACKed or automatically discarded.
At the broker delivery ceiling it remains unresolved for operator intervention.
Malformed, foreign-scope and misrouted messages are quarantined before termination;
storage/corruption failures remain retryable and do not become successful projection.
Lifecycle records/routes are not activated until M04-PR13.

## Internal read API

`serve` requires explicit loopback `EDGEAGENT_PROJECTOR_LISTEN`, for example
`127.0.0.1:8081`. All local callers are trusted as the configured operator; do not
expose/tunnel this profile. Origin headers and nonlocal Host names are refused.
There is no HTTP write surface, CORS, public login or Gateway route in this slice.

| GET path | Result |
|---|---|
| `/internal/v1/research-requests/{scope}/{id}` | 200 metadata with `scope`, `request_id`, `projection_revision`, `status`, `applied_delivery_key` |
| `/internal/v1/research-requests/{scope}/{id}/outcome` | 200 exact canonical artifact/abstention record bytes for an intact non-gapped terminal view |

Metadata status is `pending`, `artifact`, `abstained` or `gap`. Pending is a known
tracked request with no terminal fact; it is not rejection. Not-yet-observed IDs
return 404 `not_observed`, also not authoritative rejection. A pending outcome is
202; a gapped outcome is 409 `gap`; foreign scope/Origin/Host is 403, invalid ID 400,
and corrupt/unavailable storage 503 `unavailable`. No raw SQL/payload diagnostic is
returned. Reads have a five-second deadline and bounded output. Terminal queries
restore the checkpoint and revalidate its original inbox publication before returning
bytes; missing/changed proof fails closed. Internal snapshots may lag the owner and
never establish current validity, entitlement, global currency or lifecycle status.

## Verification, rebuild and rollback

`cargo xtask verify --check projector-query-contracts --python python3` runs default
credential-free HTTP refusal, local configuration, descriptor and diagnostic tests.
With loopback PostgreSQL, run
`cargo xtask verify --check projector-postgres --python python3` for real pending/
terminal/unknown/gap/corruption/availability HTTP semantics, exact bytes, duplicate
process restart and checkpoint/inbox/marker rollback at statement, deferred commit
and observed killed-worker boundaries. With PostgreSQL/NATS, run
`cargo xtask verify --check projector-broker --python python3` for actual ACK,
duplicate worker restart, both owner routes, misroute quarantine and unresolved
conflict without progress advancement. Default verification protects exact ignored
proof discovery; local-platform CI includes these opt-in suites.

Canonical SQLx assembly includes the new migration; commit generated metadata and
report missing pinned CLI/Compose prerequisites accurately. Stop intake before a
rebuild: reset disposable projection/inbox and consumer progress together and replay
authenticated retained owner publications. Do not delete inbox proofs independently,
rewrite authoritative data, skip unresolved input or assume checksum equals storage
authentication. Rollback stops serving/intake and preserves disposable state until a
coordinated rebuild. PR12 adds Gateway queries; PR13 adds lifecycle transitions.
