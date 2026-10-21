# Synthetic account admission

## TL;DR

- One PostgreSQL account lock serializes all strategies and replicas.
- The existing inbox transaction commits financial holds and outbox facts together.
- Admission catalogs/bootstrap valuations remain explicitly synthetic protections;
  P09 does not relabel them as provider quotes or fresh market marks.
- Admission itself cannot fill/release. [P09 execution](evidence-execution.md)
  adds the separately configured transactional path; public ingress stays inactive.

## Ownership and order

Read [ADR-0036](../decisions/0036-serialize-synthetic-account-admission-and-reservations.md).
Validate the typed command; require READ COMMITTED and bounded waits; lock the
root; check the verified principal grant/configuration and immutable receipt;
validate versions/quotes; read complete valuations/holds; apply P06; persist order,
reservation, risk/run checkpoints and outbox fact atomically. ACK only after commit.

Exact full duplicates retain original outcomes even after account advancement.
Conflicts/stale revisions fail permanently; canonical gaps retry. Buys reserve
upward-rounded debit plus fees; sells reserve existing long units, never proceeds.
Admission cannot release funds. P09 requires definitive terminal facts before
releasing the remainder. Missing evidence grants no capacity.

Provision new accounts only, with explicit funding and complete strategy valuations.
Bind the principal at trusted composition, not through JSON/source URIs. Retain
manifest/policy/session/quote artifacts to reconstruct context. The finite account
catalog is not a provider cache. Every future owner mutation takes this root lock.

## Verification

Run `cargo xtask verify` without services. For opt-in proofs use disposable
PostgreSQL and `EDGEAGENT_POSTGRES_URL` from [local platform](local-platform.md):

```text
cargo xtask verify --check account-admission-processes
cargo xtask verify --check sqlx-metadata
```

Private `account-worker` is qualification tooling, not a deployable. Parents own
unique schemas, restricted roles and children. Protocol barriers and database lock
observation replace sleeps. Kill before commit and after commit/before result
acknowledgement, then recover with new workers. Error/panic paths clean up fixtures.
An unavailable database fails the selected gate; ignored tests are not proofs.
Set connection-side transaction budgets before inbox insertion.

## Execution integration

The [P09 path](evidence-execution.md) adds evidence-driven pricing, liquidity,
fees and latency with balanced postings and terminal release under the same
lock/transaction. P10 still owns expiry/settlement. Provider,
public authentication and HA integration remain later work.
Admission receipts retain their original order revision 1; P09 persists the full
later lifecycle journal separately. Active holds retain their held-only constraint;
definitive terminal journal facts remove active holds without erasing the original
admission, decision receipts or financial postings. Partial-fill debits round once
cumulatively and cannot spend beyond retained cash or fee protection.
