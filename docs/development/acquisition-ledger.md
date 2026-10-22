# Durable acquisition admission and budgets

M03-PR9 implements admission/accounting. M03-PR10 owns terminal receipt/inbox/outbox
publication. No provider worker, model call or cloud deployment is enabled.
[ADR-0048](../decisions/0048-persist-acquisition-admission-and-call-budgets.md)
records this boundary.

## Ownership and recovery

The application owns `AdmissionRecord`, `AcquisitionLedger` and pure transitions.
The SQLx adapter locks account before command, samples database time after locks
and commits before returning permits. No external I/O occurs inside transactions.
Composition supplies authenticated catalogs, a bounded pool and private schema;
apply `market-data-postgres/migrations` before binding the adapter.

Canonical admission bytes bind the complete request, quality policy, listing,
rights/review witnesses and explicit limits. The digest authenticates bytes, not
permission. Identical duplicates recover the original record; changed content
conflicts. Recovery cannot extend the original deadline/retention. Recheck current
rights before every dispatch and use; decoded records are not bearer credentials.

Explicit consecutive attempt ordinals make lost-response retries idempotent.
Repeating an ordinal does not charge another attempt or reset pages. Limits span
restart: at most 3 attempts, 12 calls, 4 pages per traversal/attempt, 1 MiB per
response, 8 MiB per command and the original 300-second deadline. Account limits
only tighten: minimum 15 seconds between starts, at most 4 starts per rolling
minute across replicas. `RateLimited` reports earliest retry time, not permission.

Reserve maximum decoded response bytes and charge the call before I/O. One
unresolved permit blocks further calls on that provider account. A completed
response replaces its reservation with actual decoded bytes, including errors or
partial responses. Persist provider delays account-wide. Immutable completion
receipts prevent double refunds and clearing newer fences. Unknown outcomes
retain the full reservation.

Cancellation/deadlines deny new starts but never release unresolved permits.
There is no lease-expiry release. A trusted owner may use `QuiescedUnknown` only
after confirming the old local worker/request cannot issue more I/O; timeout or
dropping a future alone is insufficient. Otherwise leave the account blocked and
escalate. This favors bounded cost over availability, not exactly-once remote
effects. The later runtime owns worker fencing, the two-active-acquisitions cap
per worker, current-rights integration and process-kill qualification.

## Verification

- `cargo xtask verify --check acquisition-budget-contracts`: six exact model proofs
  in default verification, covering canonical records, counters, rate boundaries,
  cancellation and replay fences.
- `cargo xtask verify --check acquisition-ledger-postgres`: three PostgreSQL proofs;
  requires the owned platform's `EDGEAGENT_POSTGRES_URL`. Isolated schemas clean up
  on success/error/panic. Separate pools prove contention/reconstruction, not real
  process termination. One fixture ages rate history to avoid sleeping; pure tests
  cover exact window boundaries.
- `cargo xtask verify sqlx-metadata`: canonical Docker-backed schema freshness.
- `cargo xtask verify`: architecture, SQL policy and census mutation enforcement.

Bounded private accounting JSON rejects missing fields, unsupported versions and
inconsistent counters. It is not a public event format. Default diagnostics redact
admission bytes, URLs, account identities, licensed values and credentials; error
sources remain explicitly inspectable. PR12 owns physical deletion and retention.
