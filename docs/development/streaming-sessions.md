# Finite streaming sessions

## TL;DR

- PR16 supplies offline contracts and durable accounting, not a runnable live stream.
- Every limit is explicit; synthetic fixture values are not provider defaults.
- An uncertain connection retains its account slot until independent owner confirmation.
- No received frame, credential or vendor payload belongs in the accounting tables.

## Contracts and ownership

`StreamingSession::new` seals the complete sorted selection, common access scope,
evidence semantics, configuration digest, current authority and finite window.
It accepts intraday/core bars, trades, index levels and option snapshots only.
It neither changes daily-v1 nor grants historical reconstruction or readiness.
Authority must cover the full session. Current revisions are selected before
effective filtering so revocation cannot silently fall back to an older revision.

The owner configures `StreamingAccountPolicy` with explicit concurrent connection
capacity, lifetime starts, known opening debit, session duration, start spacing,
revision and expiry. No advertised provider allowance or daily HTTP quota is inferred.
`admit` installs exact immutable bytes; changed retries conflict. Policy/admission
retries preserve counters. No account renewal/reset path exists yet.

`StreamingLedger` exposes only worker reservation and arrival charging. Owner-only
adapter operations remain separate. After migration 0007, provision an independent
authenticated LOGIN as in the qualification guide, with:

- SELECT on stream accounts, admissions and sessions;
- UPDATE of account columns `charged_starts`, `generation`, `last_start_ms` only;
- UPDATE on stream sessions;
- EXECUTE on `edgeagent_stream_worker_allowed()` and
  `edgeagent_lock_stream_admission(TEXT)`, plus existing qualification role checks;
- no write privilege or membership capable of mutating stream owner controls.

Do not grant table-level UPDATE on accounts or any admission DML. Owner sessions
cannot serve as workers. Existing private-schema deployment/role ownership rules
still apply. Counter DML is trusted application access, not arbitrary-SQL isolation.
The guard checks table and column privileges, including NOINHERIT memberships;
column-level INSERT, UPDATE or REFERENCES must not reopen owner controls.

## Lifecycle and failure handling

1. Owner installs policy and session from authenticated current catalogs.
2. Worker `reserve` locks account/admission/session and commits start debit, capacity
   ownership and a fresh global generation before a future transport may dial.
3. Unknown commit result means unresolved ownership: do not dial or reserve again.
4. Charge each arrival before routing/deserialization using a strictly increasing
   session-local ordinal and actual observed bytes. Controls, duplicate payloads and
   filtered instruments still count. A last ordinal/size retry confirms accounting
   only; never infer that payload processing is idempotent.
5. Stop intake immediately at a cap/overage, cancellation, deadline, denial or
   uncertain accounting result. Future transport must enforce connect/idle timeouts
   and bounded allocation independently, without refreshing market freshness.
6. Cancel/withdraw does not free capacity. Owner verifies physical/provider quiescence
   and calls `confirm_quiescent` with exact account/session/generation. Repeating the
   same released generation is harmless; stale generations cannot free replacements.
7. A reconnect consumes another start, retains all frame/byte counters, and receives
   a higher generation. Neither a deadline nor a process restart resets allocations.

Account lock precedes admission and session locks. Current authority and cancellation
are checked after contention and before commit. Local cancellation denies the current
operation; owner `cancel`/`withdraw_account` provide persistent denial across replicas.
Catalog authentication/freshness remains a composition obligation. `status` observes
uncertainty after expiry but cannot authorize a dial or certify physical quiescence.

## Verification and activation gates

Run `cargo xtask verify` and focused `cargo xtask verify --check streaming-session-contracts`.
With the isolated local PostgreSQL fixture configured per the local platform guide,
run `cargo xtask verify --check streaming-session-postgres` and
`cargo xtask sqlx-metadata` under its documented isolated metadata environment.
Portable and PostgreSQL proof names are protected by the harness census.

Approved split: [PR17A](streaming-segments.md) adds immutable normalized capture/storage;
PR17B owns charged-range fencing, checkpoints, lifecycle and atomic publication. PR18
owns bounded WSS; PR19 owns a selected adapter; PR20 owns supervision/readiness and
recovery. Scoped retirement of private streaming metadata, authenticated reference
loading, restore fencing, physical disconnect evidence and provider-shared entitlement
review must exist before vendor activation. Retain these records until that reviewed
lifecycle exists; do not apply daily retirement to an unresolved stream. The daily
PR15C live qualification remains explicitly not run.
