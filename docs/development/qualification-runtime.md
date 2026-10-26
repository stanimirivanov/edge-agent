# Finite daily qualification runtime

## TL;DR

- Separate `qualification-*` commands never change synthetic/offline mode behavior.
- Independent SQL owners install complete immutable input and approve exact profiles.
- Workers cannot approve input, grant execution, reconcile unknown calls or authorize cleanup.
- A crash leaves a durable account fence; only exact-generation owner quiescence permits recovery.
- Verification is offline; PR15C needs separately authorized credentials and provider calls.

## Provisioning and commands

Apply all six MarketData migrations, inbox and outbox migrations in the private schema
using an independent migration principal. Never run migrations from a worker. Extend
[owner control](qualification-owner.md) ACLs: workers need SELECT on
`edgeagent_qualification_inputs`, ordinary DML on worker/precise-preparation tables,
and EXECUTE on `edgeagent_qualification_input(TEXT)` and
`edgeagent_qualification_input_role()`. Grant no writes on inputs or cleanup approvals,
schema CREATE, privileged role attributes or membership in an owner role. Keep database
search paths pinned. SQL controls authenticate the owner, not the truth of their reviews.
Remote PostgreSQL requires `sslmode=verify-full`; remote NATS requires `tls://`.
Loopback fixtures may use plaintext. Missing configuration reports static variable
names, never values or source error chains.

The owner verifies complete source provenance, builds the exact admitted schedule,
installs exclusive daily credits without resetting charges, and calls
`install_qualification_input(command, bytes)` followed by `approve_qualification`.
The former verifies independent SQL ownership and immutable retry; the runtime validates
the complete document before dispatch. There is no approving file decoder or default
legal grant. Store owner tooling and actual documents outside version control.

Run the MarketData binary with exactly three arguments:
`qualification-preflight PROFILE_PATH PROFILE_SHA256`,
`qualification-worker PROFILE_PATH PROFILE_SHA256`,
`qualification-relay PROFILE_PATH PROFILE_SHA256`, or
`qualification-maintain PROFILE_PATH PROFILE_SHA256`.
All require `EDGEAGENT_QUALIFICATION_DATABASE_URL` and
`EDGEAGENT_QUALIFICATION_SCHEMA`. The profile is canonical, bounded and owner-private;
its checksum is not approval. Preflight performs no credential lookup or provider work.
Worker and relay use `EDGEAGENT_QUALIFICATION_NATS_URL`; worker additionally requires
`EDGEAGENT_QUALIFICATION_STREAM` and `EDGEAGENT_QUALIFICATION_CONSUMER`.
Bind the consumer to the acquisition route, max ACK pending 1, max deliveries 1–3 and
ACK wait at least 330 seconds. Each invocation receives at most one message, never an
unbounded provider loop. A foreign command is not permission to widen this profile.

Use a pre-existing absolute private object directory through
`EDGEAGENT_QUALIFICATION_OBJECT_ROOT`; the deployment owns encryption, persistent volume
permissions and licensed-copy lifecycle. The runtime does not invent cloud storage policy.
Fresh source work requires `USE_MOCK` to be absent and
`EDGEAGENT_QUALIFICATION_LIVE_OPT_IN` to equal this exact profile digest, in addition to
independent owner approval. A configured mock flag always prevents new vendor work.
Receipt/preparation recovery does not require credentials or live opt-in.
Source work also requires explicit `EDGEAGENT_QUALIFICATION_POLL_MS` (1–100), and either
`EDGEAGENT_TWELVE_DATA_CREDENTIAL`, or `EDGEAGENT_ALPACA_DATA_KEY` plus
`EDGEAGENT_ALPACA_DATA_SECRET`. Independently verify data-only scope first. Do not print
these values, use broker endpoints or treat possession as entitlement. No test described
here needs real values. PR15C live execution requires separate operator authorization.

## Complete owner input encoding

`edgeagent.qualification-input.v1` is recursively key-sorted JSON plus LF, at most
256 KiB; duplicate/unknown fields, noncanonical bytes and alternate tuple arity fail.
IDs are 32 lowercase hexadecimal digits; revisions/epochs are positive integers.
A witness is `[id, revision, source, known_ms, source_sha256, value]`. All references
are scoped to the exact independently approved admission; no wildcard authority exists.

| Field | Explicit value encoding |
|---|---|
| `version` | Literal format above |
| `rights` | Witness history; value `[agreement_id,[effective_ms,expiry_ms],[automated,derived,canonical_storage,audience],raw_permission,canonical_retention_ms,raw_retention_ms_or_null,delete_ms,embargo_ms,status]` |
| `providers` | Witness history; value `[[effective_ms,expiry_ms],"snapshots","capture_only",status]` |
| `mappings` | Witness history; value `[instrument_id,listing_id,symbol,MIC,[start_date,end_date]]` |
| `calendars` | Complete dated witnesses; value `[MIC,date,"closed"/"regular"/"early_close",null_or_[open_ms,close_ms],tzdata_revision]` |
| `actions` | Witness history; value `[instrument_id,date,kind,terms]`; split terms `[before,after]`, cash_dividend terms `[coefficient_string,scale]`, unsupported/cancelled terms null |
| `coverage` | Witness; value `[instrument_id,[start_date,end_date],verified_until_ms]` |
| `quality` | `[revision,reference_age_ms,close_change_bps,source_difference_bps]`, exact admitted values |
| `transformation` | Explicit observation transformation revision |
| `observations` | Ascending owner-assigned `[id,revision,null]` per open day; initial revision 1 only |
| `context` | `[feed_id,epoch,effective_ms,cutoff_ms,session_date]` |
| `selections` | Witness history; value `[[effective_ms,expiry_ms],[instrument_ids]]` |
| `directories` | Witness history; value `[[effective_ms,expiry_ms],[mapping_witnesses]]`; exact symbol routes |
| `selection_ids` | `[selection_reference_id,directory_reference_id]` |
| `buckets` | Twelve Data: dated bucket witnesses `[date,[[start_seconds,nanos],[end_seconds,nanos]],calendar_id,calendar_revision]`; Alpaca: `[date,[[start_seconds,nanos],[end_seconds,nanos]],[observation_id,revision,null]]` |

Permissions are `allowed`, `denied` or `unknown`; status is `active` or `revoked`.
Only XNYS/XNAS/ARCX and the existing reviewed daily profiles qualify. All calendars,
including closed dates, are explicit. Native dates never acquire invented source instants.
The protected complete document pins actions/quality/lineage in addition to the existing
selection/schedule hash. A worker cannot replace it with a self-authored configuration.

## Failure, recovery and publication

Receipt recovery precedes current configuration and credentials. A confirmed receipt
permits ACK even after execution withdrawal; it does not grant object read permission.
Otherwise validate owner input and recover accepted preparation before source preflight.
Fresh acquisition needs full remaining-credit preflight and a bounded source attempt.
Preparation recovery consumes neither a new provider call nor a source attempt, even
after the last allowed call. Current approval, account quiescence/policy and exact worker
generation still apply. Cancelled or terminal commands cannot acquire new ownership.
Retain the returned claim and call `bind_qualification_worker` before protected
mutations. Call reservations, evidence staging and first publication lock and compare
that exact generation inside their SQL transactions. Unbound/stale views cannot mutate;
the unbound qualification ledger still supports assessment and receipt recovery.
Every new page uses the existing durable permit ledger; no retries, hidden pagination,
credit resets or fallback feeds exist. Provider spacing is explicit durable policy.

Precise and native accepted preparations survive PUT/COMMIT uncertainty. Recovery
retains original timestamps and observation IDs and reruns authority/quality. Incomplete
traversals without accepted preparation do not silently refetch. A lost/failing run
retains its account marker; owner tooling must stop old processes, confirm actual
provider/write quiescence for the exact `edgeagent_qualification_workers.generation`,
and call `confirm_qualification_worker_quiescent`. Pending calls and retired account
blocks require their separate existing owner reconciliation. Nothing refunds usage.

The worker ACKs only after terminal receipt/inbox/outbox COMMIT or durable denial.
Malformed/conflicting work commits quarantine evidence before terminal broker settlement;
other failures leave retryable delivery and durable fences. Use a dedicated qualification
consumer/queue containing only the intended profile's command, not unrelated production
work. SIGINT/SIGTERM stop intake, cancel cooperatively and allow
15 seconds to finish; dropping work is not physical quiescence. Intake is bounded at
30 seconds, work at 300 seconds, SQL statements at 5 seconds and object work at 15 seconds.
The relay publishes at most 16 outbox attempts using the established leased relay with
three publication attempts and 1–60 second retry backoff. Broker credentials must have
only the relevant route permissions. Relay can deliver historical intent after withdrawal.

## Authorized retirement and restore

Commit scope revocation/expiry retirement before deleting anything. The independent
owner inspects `pending_cleanup`, verifies all old writers/backend operations have
stopped, and calls `approve_qualification_cleanup(item, evidence_version)` for that exact
inventory. `qualification-maintain` uses the owner LOGIN to load this approval, deletes
only approved descriptors and marks cleanup complete only after all deletes confirm.
Unknown/mismatched approval fails closed without starving other approved batch items;
no worker callback grants permission. Approval
bytes disappear with completed inventory. Account blocks are not cleared by maintenance.

Restore readiness remains an independently reviewed control: reconcile current denials,
account uncertainty and all licensed copies before intake. Never restore approval as a
substitute for present owner confirmation. WAL/backup/cloud erasure needs deployment
evidence. Roll forward with migration 0006 before this binary; do not reactivate older
grant-ignoring binaries. Stop intake before rollback and preserve compatible maintenance.

## Verification

Run `cargo xtask verify`, the protected `qualification-owner-postgres` gate and canonical
`cargo xtask sqlx-metadata` against an isolated local fixture. Existing offline protocol,
storage and native publication proofs remain applicable. New tests exercise both owner
document formats, immutable input, independent SQL roles, persistent claims, stale owner
confirmation and exact preparation recovery. These are not live provider qualification.
See [ADR-0063](../decisions/0063-compose-owner-authorized-finite-vendor-qualification.md).
