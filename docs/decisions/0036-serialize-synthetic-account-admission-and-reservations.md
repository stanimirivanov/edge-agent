# ADR-0036: Serialize synthetic account admission and reservations

- Status: Accepted
- Date: 2026-10-20
- Milestone: M07 - Dry-run execution
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- P08 serializes all strategies and replicas on one simulated-account root.
- The existing inbox transaction commits admission, reservations, P06/P05
  checkpoints and an outbox fact together. Only commit permits acknowledgement.
- Quotes remain explicitly synthetic until P09; no qualified fills or live route.

## Context

In-memory risk cannot coordinate workers sharing funds. Restart must not reset
breach latches or resample identifiers. Interim quotes need explicit protection
without representing synthetic observations as qualified provider evidence.

## Decision

Add the ExecutionSimulator-owned `simulation-postgres` adapter using the existing
inbox transactional callback. Require READ COMMITTED, finite lock/statement budgets
and root `FOR UPDATE` before authorization, receipt lookup and authoritative reads.
One fenced writable PostgreSQL primary owns each account; no active-active writes.

Bind a verified principal outside JSON; a CloudEvents source is not authentication.
Check its grant under the lock. Freeze an account-scoped run, policy, session and
1..=4096 service-owned synthetic quotes. Quotes bind full order/instrument terms,
canonical input, virtual time/validity, price ceiling and explicit total fee cap.
Callers cannot supply funding, marks or decision time.

Buys reserve ceil(price x quantity x pinned option multiplier in USD cents) plus
fees. Checked arithmetic rejects overflow. Sells reserve existing strategy-owned
long units and fees, never proceeds. Cash and account limits span every strategy;
strategy caps apply too. Read complete valuations and all outstanding holds.
Missing, stale or tampered evidence fails closed. Observed P06 breaches persist
across restart; projected increase denial does not latch a healthy account.

Require contiguous canonical input and optimistic account revisions. Gaps retry;
stale/conflicting revisions fail permanently. Both admission and business denial
consume an input and append an immutable result. Check committed receipts before
new revisions/expiry: exact full requests retain results; changed content conflicts.
Persist P05 continuation including preallocated order IDs, P06 state and trace links.

Published event IDs bind account, manifest digest and deterministic run event ID.
Accounts sharing frozen inputs cannot collide in the service-wide outbox identity.

## Alternatives considered

- Worker/strategy locks cannot protect account-wide budgets across replicas.
- Mixed isolation modes introduce snapshot/retry ambiguity and are rejected.
- Unbounded orders, guessed fees and anticipated sale proceeds are not funding.
- Another driver or transaction path obscures inbox/domain/outbox atomicity.
- Public activation before authentication and P09 exposes incomplete execution.

## Consequences

Hot-account writes serialize intentionally. Catalogs, strategies and signed database
amounts are bounded. Provisioning creates new accounts only; no cash refill/reset.
Holds stay held. P09/P10 must enforce protection, apply terminal effects, release
remainders and update balanced ledgers under this root lock and transaction.
No release, fill or ledger API is added now. M07 remains incomplete.

## Compatibility and migration

Private tables and new versioned messages are additive. Existing bootstrap stays
inactive; private proof workers compose the real adapter. Apply owner inbox/outbox
and account migrations before composition. Rollback disables ingress and retains
financial evidence; never drop account/order/reservation tables to downgrade.

## Security and operations

No external calls enter transactions. Set connection-side budgets before inbox
insertion as well as transaction-local budgets. Separate migration/provisioning
identities from runtime. Proof runtime cannot edit grants, quotes, valuations,
inventory or cash. Debug omits amounts/encoded records. Checksums detect corruption,
not hostile database owners; secure backups/storage. Synthetic never means live.

## Validation

Run `cargo xtask verify` and `cargo xtask verify --check sqlx-metadata`. The protected
`cargo xtask verify --check account-admission-processes` gate discovers four real
process proofs for contention, rollback/death, committed duplicate result loss,
sticky risk, authorization and invalid inputs. Pure tests cover rounding, option
multipliers, overflow and corrupt checkpoints. These are synthetic correctness
proofs, not broker, database restart, power-loss, load or HA qualification.
