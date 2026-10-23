# ADR-0051: Bind provider page dispatch to durable permits

- Status: Accepted
- Date: 2026-10-23
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Require a newly confirmed durable call permit and fresh local authority per HTTP
page. Retain unknown reservations and completed pages through lost confirmations.
Project usage from the existing ledger. No live provider or worker is activated.

## Context

The ledger reserves calls/bytes across replicas, but a public raw HTTP session
could bypass it. Lost reservation replies, partial bodies and lost accounting
confirmations need distinct recovery actions, not another external read.

## Decision

Expose only `HttpTransport::start_metered` and `MeteredSession` for GETs. Match
the complete immutable admission. Application-owned `DispatchAuthority` supplies
fresh authenticated local catalogs/time without I/O; `DispatchCheck` validates
current witnesses and original bounds. Recheck before reservation, after its
await and before releasing data. Narrow monotonic deadlines, never extend them;
reject backward UTC samples.

Block locally before awaiting `start_call`. Only a newly returned matching permit
authorizes one GET. Unknown outcomes keep the full reservation and account fence.
Never invoke `QuiescedUnknown` here or dispatch a recovered permit. Complete body
receipt (even invalid JSON), classified statuses and proven pre-I/O rejection
reconcile known bytes. Error bodies are not read. Persist valid Retry-After even
beyond this command's deadline. Save the exact completion and page/error before
awaiting accounting. `confirm_page` retries reconciliation, never HTTP. Reconcile
after expiry/cancellation, but deny data release. Losing the entire session loses
its local body, not accounting. Exactly-once remote reads are not promised.

Usage strips identifiers/permits from confirmed ledger snapshots. Static-label
local metrics are best effort, never quota authority. Preserve existing limits.
No SQL, migration, worker or vendor changes.

## Alternatives considered

- Raw GET: documentation cannot enforce durable charging.
- In-memory limiter: loses the consistent account view across restarts/replicas.
- Lease-expiry release: cannot prove the former request is quiescent.
- Re-fetch after lost accounting: burns another call for a received page.
- Vendor SDK: supplies neither vendor selection nor durable quota policy.

## Consequences

Known pages survive confirmation retries in one session. Unknown requests block
availability until the runtime proves quiescence. Catalogs/time remain trusted
composition inputs, not authenticated by a URL/digest. Offline stays network-free.

## Compatibility and migration

BREAKING Rust adapter API: remove exported unmetered session/start; consumers must
provide the ledger and current authority. No production consumer exists yet. No
wire/persistence changes. Revert needs no migration but reopens the raw bypass.

## Security and operations

Do not log identities, URLs, credentials, payloads or source chains. Metrics use
static labels. No retries, sleeps, failover, quota increases, external I/O inside
transactions, model calls or broker paths. PR13 owns runtime authority/quiescence;
PR14 owns process-death qualification. Unknown requests remain charged and fenced.

## Validation

`cargo xtask verify --check metered-provider-dispatch` protects six loopback/model
tests and an independent census mutation test. `provider-http-contracts` keeps its
six existing proofs without duplicate runs. `cargo xtask verify` includes both,
architecture checks and the negative rustdoc API example. These do not qualify
PostgreSQL/process faults or live providers. Run supply-chain checks for the
existing pinned contracts/metrics dependency edges.
