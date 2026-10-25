# ADR-0062: Require independent PostgreSQL owner grants for qualification

- Status: Accepted
- Date: 2026-10-25
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Separate authenticated SQL owner control from restricted worker execution.
- Bind grants to exact finite profiles and five independently reviewed controls.
- Check grants inside reservation and first-publication transactions, not from a cached pass.
- Keep policy installation and unknown-call/account reconciliation owner-only APIs.
- Deliver PR15B1 before PR15B2 runtime composition; keep PR15C live proofs separate.

## Context

ADR-0061 defines preflight, not a reusable execution grant. The maintainer approved
PostgreSQL owner control, independent worker privileges and generation-matched owner
confirmation. Combining this boundary with vendor factories, reference decoding and
process lifecycle changes would obscure security review. It needs actual SQL role proofs.

## Decision

Migration 0005 creates a grant relation with no PUBLIC privileges. Owner connections
record exact canonical profiles and five review identities, known times and expiries.
SQL write privilege authenticates the operator, not the truth of legal, credential-scope,
reference, fencing or cleanup assertions. Operators verify those facts independently.
Exact API retries preserve the grant; the approval API cannot reverse withdrawal.
Retain both approving current/session principals. Deny execution by that session principal
or members of the approving role, including after privileges have changed; role changes
must not launder self-authored approval into independent execution.

`for_qualification` binds the existing ledger to one profile. Attempt/call transactions
lock scope/account/command before the grant, check current approval/window using database
time, compare the complete installed daily policy and require conservative remaining
credits. Existing accounting reserves quota; there is no parallel limiter. Resample after
approval contention and recheck validity after writes, before COMMIT. Denial rolls back
all changes. No external I/O occurs inside transactions.

First terminal publication checks the same grant inside its receipt/inbox/outbox transaction
and before COMMIT. Withdrawal serializes with shared grant locks: work linearizes before
withdrawal or fails after it. Historical receipt recovery remains possible, but grants no
evidence-read permission. Observed-response accounting may complete after withdrawal.
Unknown-call confirmation, daily-policy installation and retired-account release require
owner-capable connections; existing generation/physical-quiescence checks still apply.

PostgreSQL locking SELECT requires UPDATE privilege, which workers must not receive.
A single owner-defined SECURITY DEFINER function performs only a primary-key locking
read. SQL-standard function-body binding fixes its relation OIDs at definition time;
execution fixes search path to `pg_catalog`. Revoke PUBLIC execution and explicitly grant
its execution to workers, preserving read-only approval ACLs.
An additional read-only INVOKER function checks actual caller/session privileges against
the early-bound control relation; implicit temporary tables cannot spoof owner permission.
Its execution is also explicitly provisioned, never granted through PUBLIC.

Qualified execution rejects owner-capable worker connections: schema CREATE, control-write
privileges, privileged roles and NOINHERIT membership capable of SET ROLE. Check both
current and session principals to reject an administrator masquerading as a worker.
A plain ledger cannot bypass a protected grant. Restricted workers require a bound profile;
unprotected legacy invented/local owner workflows remain usable after migration.

These are application transaction guards plus database ACLs, not sandboxing against a
compromised service issuing arbitrary SQL against ordinary accounting tables. Owner/admin
database access remains trusted and can override state outside these APIs.

## Alternatives considered

- Self-approved files or preflight checksums: neither authenticates an independent owner.
- Worker startup provisioning roles/grants: collapses the permission boundary.
- New OIDC/signature infrastructure: deferred; PostgreSQL authentication already exists.
- One security/runtime PR: split into PR15B1 and PR15B2 for independent review.

## Consequences

Reservation and publication consume this foundation. PR15B2 still owns authenticated full
reference/configuration loading, actual worker/write fencing, vendor factories, recovery,
maintenance and authorized cleanup composition. PR15B1 adds no CLI, credential lookup,
source factory or live activation. PR15C still requires explicit live permission.

## Compatibility and migration

Apply 0005 after 0001–0004 before deploying new clients. Provision owner and worker roles
separately; workers receive SELECT only on grants, no schema CREATE or owner-role membership.
Never migrate with the worker login. Promote the adapter's existing domain development
dependency for production clock validation and update both architecture sensors. No new
external dependency, event-schema change or dependency version resolution is introduced.
Restricted legacy API behavior tightens deliberately.

Stop intake before rollback. Keep the grant relation and a compatible binary for outstanding
protected jobs: older binaries ignore approval and must not regain execution privileges.

## Security and operations

The admission foreign key cascades erasure of private grant/profile/review bytes on
retirement. Existing opaque denial/account-generation records remain authoritative.
Execution withdrawal does not itself revoke licensed retained reads; legal withdrawal also
uses scoped evidence denial. Physical deletion still needs owner authorization and actual
quiescence. WAL, backups, privileged compromise and distributed process fencing remain
separate controls. Diagnostics redact source details.

## Validation

Protect four PostgreSQL proofs with independent census/mutation tests: authenticated LOGIN
roles, denied approval writes and NOINHERIT escalation, exact profiles, withdrawal/reservation
contention, atomic counters, terminal rollback/recovery and retirement erasure with denied
worker reconciliation. Regenerate checked SQLx metadata against an owned local fixture.
Run preflight regressions, existing ledger/publication/retention proofs and complete default
acceptance. No provider is contacted or qualified.
