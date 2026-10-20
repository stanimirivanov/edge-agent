# Synthetic account admission

## TL;DR

- One PostgreSQL account lock serializes all strategies and replicas.
- The existing inbox transaction commits financial holds and outbox facts together.
- Quotes/valuations remain explicitly synthetic until P09 qualification.
- No live route, fill, ledger, release or public ingress is enabled.

## Ownership and order

Read [ADR-0036](../decisions/0036-serialize-synthetic-account-admission-and-reservations.md).
Validate the typed command; require READ COMMITTED and bounded waits; lock the
root; check the verified principal grant/configuration and immutable receipt;
validate versions/quotes; read complete valuations/holds; apply P06; persist order,
reservation, risk/run checkpoints and outbox fact atomically. ACK only after commit.

Exact full duplicates retain original outcomes even after account advancement.
Conflicts/stale revisions fail permanently; canonical gaps retry. Buys reserve
upward-rounded debit plus fees; sells reserve existing long units, never proceeds.
Cancellation cannot release funds in this slice. Missing evidence grants no capacity.

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

## P09 hand-off

P09 qualifies evidence-driven pricing, liquidity, fees and latency. Fills must
respect retained protection and update balanced ledgers/release terminal remainders
under the same lock and transaction. P10 qualifies expiry/settlement. Provider,
public authentication and HA integration remain later work.
P09 must migrate the admission-only order-revision and held-reservation constraints
before introducing later lifecycle revisions or terminal reservation states.
The buy reservation rounds total-order notional once. P09 must keep cumulative
partial-fill debits within that protection; independently rounding every partial
fill upward can exceed the hold. Qualify cumulative rounding or an explicit extra
allowance before activation, and never spend beyond the retained total fee cap.
