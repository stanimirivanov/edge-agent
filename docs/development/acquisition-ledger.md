# Durable acquisition admission and budgets

## TL;DR

- Reserve reviewed daily credits with call/byte permits in one account transaction.
- Deny missing, expired, unpriced or exhausted authority; never refund uncertainty.
- Keep offline replay quota-free and pending-call recovery owner-confirmed.
- Legacy account state remains readable but needs explicit daily installation for I/O.

M03-PR9 implements admission/accounting. M03-PR10 adds atomic
[terminal publication](acquisition-publication.md). No provider worker, model call
or cloud deployment is enabled.
[ADR-0048](../decisions/0048-persist-acquisition-admission-and-call-budgets.md)
records this boundary.

M03-PR11B binds every HTTP page to this ledger through `MeteredSession`; see
[provider HTTP](provider-http.md) and
[ADR-0051](../decisions/0051-bind-provider-page-dispatch-to-durable-permits.md).
No second limiter, lease release or production worker is added. Current local
authority is checked before dispatch and data release. Usage reads confirmed
ledger state without exposing permit or account identifiers.

## Ownership and recovery

### Reviewed daily credits

[ADR-0055](../decisions/0055-reserve-reviewed-daily-credits-with-account-permits.md)
adds account-wide fixed UTC-day credit reservations. External dispatch now denies
missing/expired policy, an unpriced endpoint or exhausted credits. Offline replay
does not call `start_call` and consumes no external quota. Existing minute ceilings
remain unchanged; a provider's advertised limit never raises them.

After admission creates the account, trusted composition calls
`PostgresAcquisitionLedger::configure_daily_credits` with a checked
`DailyCreditPolicy` and explicit current-day opening debit. The review binds
revision, positive allocation, exclusive expiry and up to 16 endpoint prices.
Each price must cover the maximum allowed billable request shape, not merely a
nominal URL: symbol counts and endpoint weights matter. Only reviewed 00:00 UTC
reset schedules are supported. No guessed defaults exist.

Installation requires confirmed worker quiescence and known prior/shared usage.
Reserve the allocation exclusively for this ledger; fence other consumers or
enforce disjoint allocations. Unknown sharing/usage means do not install or dispatch.
Keys/replicas are not separate allowances. This API does not authenticate reviews
or discover provider usage. An explicit zero is an owner assertion, never a default.
Identical installation is idempotent without resetting usage; any changed policy
or opening debit conflicts. Review replacement/renewal needs a later non-resetting
owner protocol before long-lived activation.

Credits commit with the call/byte permit under the same account lock. Failed SQL
rolls everything back; errors/uncertainty/cancellation never refund committed
credits. A new UTC day resets only a quiescent credit window, not minute history,
Retry-After or pending work. Completion across midnight carries the endpoint price
into that day too. Owner confirmation after retired endpoint erasure closes the
entire confirmation-day allocation before releasing the exact opaque block.
Clock regression behind committed accounting denies dispatch. Expiry still allows
reconciliation, not new calls.

`daily_credit_usage` reads persisted revision/day/charged/allocation/expiry without
advancing a window or granting dispatch. Account-v1 remains readable but unconfigured;
installation writes private v2. Admission/CloudEvents/SQL shapes stay unchanged.
Older code rejects v2: disable dispatch for rollout/rollback and preserve all bytes;
never downgrade/reset accounting. Provider activation and authenticated reviews
remain separate. Local reservations cannot guarantee remote billing or zero 429s.

### Account transactions and recovery

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

- `cargo xtask verify --check acquisition-budget-contracts`: ten exact model proofs
  in default verification, covering canonical records, counters, rate boundaries,
  cancellation and replay fences.
- `cargo xtask verify --check acquisition-ledger-postgres`: four PostgreSQL proofs;
  requires the owned platform's `EDGEAGENT_POSTGRES_URL`. Isolated schemas clean up
  on success/error/panic. Separate pools prove contention/reconstruction, not real
  process termination. One fixture ages rate history to avoid sleeping; pure tests
  cover exact window boundaries.
- `cargo xtask verify sqlx-metadata`: canonical Docker-backed schema freshness.
- `cargo xtask verify`: architecture, SQL policy and census mutation enforcement.

Bounded private accounting JSON rejects missing fields, unsupported versions and
inconsistent counters. It is not a public event format. Default diagnostics redact
admission bytes, URLs, account identities, licensed values and credentials; error
sources remain explicitly inspectable. The [retention boundary](evidence-retention.md)
erases expired command metadata while retaining opaque account blocks for unknown
calls. After erasure, only generation-fenced owner quiescence can clear the block;
ordinary completion/recovery never resurrects the retired command.
