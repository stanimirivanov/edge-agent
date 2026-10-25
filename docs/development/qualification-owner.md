# PostgreSQL qualification owner boundary

## TL;DR

- PR15B1 adds owner grants and atomic reservation/publication guards, not a vendor worker.
- Separate SQL owner control from a real restricted worker LOGIN; SET ROLE is insufficient.
- Workers cannot approve themselves or use API callbacks to confirm unknown-call quiescence.
- Private grant bytes disappear with retired admissions; opaque denials/blocks remain.
- PR15B2 owns runtime/configuration/fencing and cleanup; PR15C owns opt-in live proofs.

## Provisioning and trust

Apply `market-data-postgres` migration 0005 after 0001–0004 in the service-private
schema before deploying clients. It creates `edgeagent_qualification_grants` with no
PUBLIC privileges, not roles or credentials. The independently controlled owner
connection owns migrations, admission and account policy installation. It alone receives
approval INSERT/UPDATE and verifies external legal/operational facts before approval.
SQL privilege authenticates that operator, not the truth of their assertions.

Give workers a different authenticated LOGIN, minimum ordinary ledger/publication
permissions, SELECT **only** on grants and EXECUTE on `edgeagent_lock_qualification_grant(TEXT)`
and `edgeagent_qualification_role()`.
PostgreSQL row locks normally require UPDATE privilege. This one SECURITY DEFINER function
performs only a primary-key locking read; its SQL-standard body binds relation OIDs at
definition time and fixes the runtime search path to `pg_catalog`. It grants no write
capability. The INVOKER role-check function also binds the genuine control relation at
definition time; an implicit temporary table cannot manufacture owner permission. Neither
function grants writes, and PUBLIC execution is revoked. Deny schema CREATE, control writes,
privileged role attributes and membership in any approval-writing role, even NOINHERIT membership.
Checks cover both current and session principals: an administrator followed by SET ROLE
could RESET ROLE and is not a restricted login. Pin the connection search path to the
owner-provisioned schema. Worker startup must not migrate, create roles or approve itself.

Ordinary ledger DML remains trusted service access. The ACL prevents owner-grant writes;
application guards protect the existing APIs. This does not sandbox a compromised service
issuing arbitrary SQL against its accounting tables. A stored-procedure-only accounting
redesign is not implied. Owner/admin database access remains a privileged boundary.

## Owner lifecycle

`approve_qualification(profile, controls)` requires controls in this order: data-only
credential scope, reference provenance, runtime fencing, retention/cleanup, restore/consumer
compatibility. Each has independent evidence/version, inclusive known time and exclusive
expiry covering the original licensed deletion ceiling. Future-known, expired or reordered
reviews fail. These are owner-reviewed assertions, not provider declarations, defaults or
automatic credential introspection. Profiles never contain actual credentials.

The grant retains exact bounded canonical profile/review bytes and approving SQL principal.
It also retains the approving session principal. Execution denies that same LOGIN or its
membership in the approving role, even if approval privileges were subsequently removed.
Identical retries preserve it; changed inputs conflict. `withdraw_qualification` withdraws
the exact profile without refunding usage, clearing uncertainty or deleting objects. The
approval API cannot reverse withdrawal. Administrators can override SQL state and must be
controlled separately. Fresh work after withdrawal needs a new acquisition/review.

Execution withdrawal differs from legal revocation. Legal withdrawal also uses existing
scope-denial/retirement APIs to deny retained reads. Retirement erases grant/profile/review
bytes through the admission foreign key, retaining existing opaque denial/account-generation
records. This does not promise erasure from WAL, backups or other licensed copies.

## Worker transactions and recovery

`PostgresAcquisitionLedger::for_qualification(pool, profile)` binds one exact command;
construction performs no I/O or approval. Attempt/call transactions retain scope/account/
command lock order, then lock the grant. Missing/withdrawn/different profiles or privileged
worker sessions fail closed. The legacy constructor cannot execute a protected acquisition;
restricted roles cannot use it for unapproved work. Legacy invented/local owner workflows
with no grant still work after migration.

Use database time after contention; compare the **complete** installed daily policy,
including endpoint prices, and require worst-case remaining credits. Preserve account-wide
pending calls. Existing accounting owns spacing, rate, bytes and atomic reservation. Recheck
profile validity after writes, before COMMIT. Denial rolls back state. No external I/O occurs
inside a transaction. Shared approval locks serialize withdrawal: starts commit before it
or fail after it. Already-reserved external operations may still begin or finish;
withdrawal is not proof of provider/write quiescence.

First receipt/inbox/outbox publication checks the current grant inside its transaction and
before COMMIT. Receipt-first recovery may replay original bytes after execution withdrawal;
it starts no source work and grants no object-read permission. Observed-response accounting
can finish after withdrawal. `QuiescedUnknown`, daily-policy installation and retired-account
release require owner-capable connections. A worker-supplied `CleanupAuthority` cannot grant
those API privileges; exact-generation and independent quiescence checks still apply to
owners. A lease expiry or timeout never clears uncertainty.

## Verification, rollout and remaining work

`cargo xtask verify --check qualification-owner-postgres` executes four protected tests
against the documented isolated loopback PostgreSQL fixture, using separate actual LOGIN
roles, owned schemas and SQL faults. Test passwords are generated fixture identities, not
real provider credentials. Independent census mutation tests reject missing, renamed,
duplicate or additional proofs. Default `cargo xtask verify` protects their discovery but
does not execute PostgreSQL tests; run the T3 gate separately. Migration/query changes also
run the canonical [SQLx metadata proof](sqlx-metadata.md).

Stop intake before rollback: older binaries ignore grants and must not receive protected
work credentials. Keep a compatible binary/schema for outstanding work and retention.

PR15B2's [runtime](qualification-runtime.md) supplies complete owner input, durable
provider/write ownership, both finite daily protocols, recovery/publication and exact
owner-approved cleanup. The offline worker stays invented-only. These offline proofs
do not live-qualify either provider. See [the plan](plans/m03-acquisition.md)
and [ADR-0062](../decisions/0062-require-independent-postgresql-owner-grants-for-qualification.md).
