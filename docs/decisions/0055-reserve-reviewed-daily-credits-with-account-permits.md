# ADR-0055: Reserve reviewed daily credits with account permits

- Status: Accepted
- Date: 2026-10-24
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Reserve explicit endpoint credits in the existing account transaction before I/O.
Use fixed UTC days, fail closed without a reviewed allocation, and retain charges
after errors or uncertainty. Offline sources remain quota-free; no vendor activates.

## Context

Rolling-minute starts do not bound daily consumption. Per-process counters or
another limiter would multiply allowances across replicas or lose them on restart.
A provider's advertised allowance is not necessarily this application's allocation:
other keys, users and applications may share the same subscription. Review must
cover both account ownership and the maximum billable shape behind each endpoint.

## Decision

The trusted owner installs an immutable `DailyCreditPolicy` after admission creates
the account. It binds a positive revision, exclusive expiry, daily allocation and
1–16 positive endpoint costs. Endpoint identity represents a reviewed upper bound
over every allowed request shape, including symbol multiplicity and endpoint weight.
No advertised provider defaults or API-key-derived quota identities exist in code.
Unknown costs and unconfigured/expired policy deny external dispatch.

Installation requires owner-confirmed quiescence and an explicit opening debit
covering all prior current-day consumption, including shared consumers. The owner
must exclusively reserve this allocation: either fence all other account consumers
or reserve a disjoint, enforceable allocation for them. Unknown shared usage denies
installation. This is a trusted composition obligation, not proven by constructing
a policy or setting a zero opening debit. Account aliases must not multiply quota.
The ledger rejects known pending work and opening debits below retained same-day
starts, but cannot independently discover historical/external usage. Identical
installation retries retain counters; changed policy/opening debits conflict.

`start_call` charges endpoint credits with calls, maximum bytes and the account
permit under the existing account-before-command locks and database clock. It
returns only after commit. Failures roll back all reservations. Existing spacing,
rolling starts, provider Retry-After, cancellation, terminal and opaque retirement
blocks remain authoritative. No extra table, process-local limiter or remote I/O
appears inside transactions. Usage reads persisted accounting, not permission.

Credits never refund, even for known pre-I/O rejection, failed response, cancellation,
lost confirmation or `QuiescedUnknown`. Command byte reconciliation remains separate.
At 00:00 UTC a quiescent account can start a new credit window; clock regression
behind committed accounting fails closed. Day rollover cannot clear a pending permit.
A call reconciled on a later day also charges its endpoint price in the completion
day: the remote debit may have crossed midnight. Retiring a pending command erases
its endpoint identity, so exact-generation owner confirmation conservatively closes
the whole confirmation-day allocation before unblocking, atomically. Stale confirmation
rolls back that accounting; identical confirmation cannot charge a later day again.

## Alternatives considered

- Only four starts/minute: does not enforce daily credits or weighted endpoints.
- Independent daily limiter: cannot atomically reserve with the existing permit.
- Refund on timeout/error: cannot establish whether the provider consumed credit.
- Assume midnight releases work: paused workers can resume and overlap the new day.
- Infer free-plan settings, shared consumption or quota from key count: unreviewed.
- General policy replacement/reset API: expands the owner-authorization contract;
  defer a non-resetting review-renewal protocol until required for activation.

## Consequences

Conservative overcharging favors bounded cost over availability. Expired reviews
and unknown consumption stop dispatch until the owner can establish a safe policy;
new accounts or API keys must not become quota-reset workarounds. Day-crossing
recovery can consume two windows, and retired-call confirmation closes a full day.

## Compatibility and migration

Admission-v1/v2, receipts, public CloudEvents and SQL schemas are unchanged. Private
account-v1 bytes still decode and retain their minute/pending history, but have no
daily authority. Explicit installation upgrades that account's bounded JSON to v2.
Old binaries reject v2 rather than silently ignoring it. No SQLx statement shape or
offline metadata changes are required: reuse the existing checked statements.

BEHAVIOR-ADJACENT: existing external-call callers now require explicit installation;
synthetic attempts/publication/replay do not. Apply code with dispatch disabled,
authenticate reviews and reconcile/fence former workers before installing budgets.
Do not rewrite old admissions, reset accounts or downgrade v2 bytes for rollback.
Stop dispatch and retain accounting until a forward-compatible recovery is available.

## Security and operations

This bounds committed local reservations, not exactly-once remote billing or a
guarantee against provider 429 responses. External clock/billing semantics and
non-UTC resets require separate qualification; unsupported schedules deny activation.
Review renewal/replacement and authenticated live configuration remain future work.
Policy expiry stops calls, not reconciliation. Usage exposes bounded counters only;
default diagnostics omit endpoint/account identity and private bytes. No provider,
credential, model, paid account or live qualification is enabled by this decision.

## Validation

Protected portable acquisition-budget proofs cover legacy decoding, explicit
installation, weighted boundaries, expiry, unknown prices, overflow, clock regression,
no mutation on denial, midnight carry and fence retention. The local PostgreSQL
proof injects an account-write failure after the command update, races independent
pools, reconnects after a committed reservation and proves immutable initialization
and charges. Existing retention tests prove stale-generation credit rollback and
conservative quiescent release. Process-death quota proofs retain zero-charge offline
isolation. No test calls a provider or uses licensed observations.
