# Authorized recoverable quarantine replay

## TL;DR

- Run `cargo xtask verify --check event-spine-processes` with the isolated
  PostgreSQL profile and Docker or the pinned native NATS server. Missing
  prerequisites fail. The suite owns and cleans up its schemas, roles and broker.
- The finite tooling worker implements replay composition, not a product
  operator endpoint. [ADR-0028](../decisions/0028-compose-authorized-recoverable-quarantine-replay.md)
  records the trust boundary and compatibility contract.
- Authenticate as the owning operator; never accept actor identity in JSON.
  Commit authorization and dispatch intent together, then publish from a
  separate restricted dispatcher with no open database transaction.
- Retry an uncertain result with the same request ID. Original envelope bytes,
  CloudEvents identity and subject never change. Retain quarantine and audit.
- `idle` is not proof of completion: work may be leased or exhausted. Inspect
  dispatch state and receipts before taking further action.

## Scope and credentials

This is synthetic M02 tooling and a minimal **[Prerequisite from M10]**.
An administrator creates fresh owned schemas, schema-scoped operator/dispatcher
logins and registry-derived broker grants. Runtime workers cannot authorize
replay. Operators cannot dispatch; dispatchers cannot authorize or mutate payloads.
The actor comes from PostgreSQL `session_user`, checked against the configured
schema, not a request field or `SET ROLE`. Configuration and the administrator
remain trusted. A compromised owner/admin is outside this boundary; these grants
are not a database-enforced state machine against arbitrary SQL from a malicious
dispatcher. Production IAM, approval policy and dual control remain M10 work.

The fixture uses public `fixture-only` passwords on exclusively owned local
resources. Never use them in a deployment. Workers receive database URLs in a
private environment, not command-line arguments; errors omit sources and payloads.
The parent clears unrelated environment variables and enforces readiness/deadlines.

## Request and recovery procedure

1. Inspect retained evidence in its owning schema using an authorized read path.
   Confirm the original consumer/transport key or source/message ID, failure
   reason and remediation. Do not dump payloads into logs or issue comments.
2. Choose a bounded replay request ID and reason code. An inbound request has
   `direction`, `request_id`, `consumer_name`, `delivery_key`, `reason`. An
   outbound request substitutes `message_source` and `message_id` for the two
   inbound target fields. Extra fields, including `requested_by`, fail.
3. Invoke `conformance-worker replay-authorize` with the owning operator database
   login, configured `EDGEAGENT_PROOF_SCHEMA`, `EDGEAGENT_PROOF_NAMESPACE` and
   `EDGEAGENT_PROOF_COMPONENT` (`gateway`, `research`, or `auditor`). These are
   trusted composition-root settings, not an end-user authorization API.
   Wait for `ready`; send byte `c` followed by JSON and EOF (maximum 4 KiB).
   No broker connection is needed. Fixture-only byte `r` proves rollback.
4. Inbound `authorized` means audit plus immutable dispatch snapshot committed,
   not published or applied. Re-entering the identical request is idempotent;
   changing target, reason or actor under the same ID fails. Unsupported,
   malformed, foreign-subscriber or wrong-subject evidence cannot be repaired
   through replay. Authorization validation failure rolls back the audit too.
5. Invoke a separate `replay-dispatch` worker with the owning dispatcher login,
   matching component and `EDGEAGENT_NATS_URL`; after `ready`, send `c` and EOF.
   It claims one eligible intent and commits before network work. Claims spend
   one of three attempts, lease for ten seconds and increment a generation.
   Broker operations are bounded to five seconds; no automatic background loop
   or publication from `Drop` exists.
6. `replayed` / `replay_duplicate` mean a valid persisted broker receipt and
   fenced database completion committed together. They do not mean the consumer
   applied a domain transition. Consumer inbox and service-owned semantic guards
   make an already committed original identity a no-op.
7. After a crash/unknown result, retain the same request. Wait for the lease to
   expire and run another dispatcher. Never edit leases in an operator runbook;
   only isolated tests expire them to coordinate fault injection. A current
   generation can finish; an older holder cannot overwrite its result. After
   three attempts, retain unresolved work for inspection; do not reset counters
   or infer failure from an absent receipt. A new request is a new intentional
   replay and requires a fresh operator decision, not an automatic retry policy.

Outbound `released` / `already_released` compose the existing adapter's atomic
audit/release operation. They reset only the attempt budget; retained claim
generation fences survive. The ordinary owning relay performs publication.
Authorization rollback leaves both audit and quarantined row unchanged. Do not
update product tables directly to bypass that API.

## Inspect outcomes without exposing payloads

Inside the owning synthetic consumer schema, an authorized inspector can use:

```sql
SELECT d.request_id, d.attempt_count, d.generation,
       d.lease_expires_at, d.published_at,
       r.stream, r.sequence, r.duplicate
FROM conformance_replay_dispatch d
LEFT JOIN conformance_replay_receipt r USING (request_id)
ORDER BY d.request_id;
```

An unpublished intent with attempts below three and an absent/expired lease is
eligible. An active lease is unresolved in flight. Three attempts with no
publication receipt is exhausted/unresolved, not confirmed non-delivery.
Receipts are append-only for the restricted roles; quarantine and original audit
remain retained. Table retention/archival policy and operator UI are not provided.

## Transport identity and subscriber effects

Inbound publication uses `replay/<owned-schema>/<request-id>` as `Nats-Msg-Id`,
separate from the ordinary producer's identity. It sends the exact retained body
on the original subject. A restart within the broker's deduplication window may
return a duplicate receipt; beyond that window another delivery is possible.
Durable inbox/domain deduplication, not a finite broker window, protects effects.
An intentional new request permits a new publication without changing domain ID.

Original-route event replay reaches **all** subscribers, not exclusively the
quarantining subscriber. Each subscriber must retain its own deduplication
evidence. The replay broker principal has only incoming exact-subject publication
and a private receipt inbox, not intake, ACK or topology authority. Its inbox is
a sibling of the runtime inbox, outside the runtime wildcard.

## Proof coverage and limits

The protected process target adds four cases: authenticated scope/atomic
rollback/conflicting re-entry and SQL denials; inbound restart/result loss,
stale fences and exhausted attempts with one domain transition; original-route
fan-out and observed broker permission denial; outbound audited budget reset
with stale claim rejection. The receipt-loss barrier (`p`, then `persisted`)
exists only in tooling and waits outside a transaction; the parent kills/reaps
the child or releases it with `c`. It proves application result loss after a real
broker receipt, not lost wire acknowledgements or a broker/database restart.

Default verification compiles and discovers these ignored service-backed tests;
the T3 gate actually executes them. Checked SQL and offline metadata include
the disposable replay tables. Use fresh fixtures: no mixed-version worker or
retained synthetic-schema upgrade is supported. Product migrations, portable
ports and deployed service shells are unchanged. No models or trading run.
