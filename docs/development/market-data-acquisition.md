# Acquisition authority and provider conformance

## TL;DR

- `market-data-application` checks reviewed authority before and after provider I/O.
- Exact scope and current rights are mandatory; unknown never means permitted.
- Capture and retained historical knowledge are distinct. Never backdate receipts.
- No live provider, runtime, HTTP, persistence or physical-deletion implementation exists here.

## Application boundary

[ADR-0044](../decisions/0044-gate-acquisition-on-reviewed-rights-and-provider-authority.md)
defines the application-owned contracts. Build an explicit `AcquisitionRequest`
with typed command/principal/provider-account/audience/storage identities, source,
instrument, reviewed endpoint ID, evidence contract, date range, knowledge mode,
initial admission/deadline, remaining limits and raw-retention intent.

`AuthorityCatalogs::admit` uses authenticated retained rights and provider-review
histories. It selects the latest revision known at the supplied trusted time
before scope/effective filtering. Exactly one record of each kind must match.
Unknown, denied, revoked, expired, future-known and conflicting authority rejects
admission. Renew records with revisions; competing matching grant IDs fail closed.
Constructing `RightsRecord` or `ProviderReview` does not prove source authenticity,
data-license approval, actual feed quality or account access.

The `DailyProvider` port accepts `AuthorizedAcquisition`, not unreviewed input.
The adapter must enforce positive connect/whole-request limits (at most 5/15
seconds), remaining calls/pages (at most 12/4), response/command decoded bytes
(at most 1/8 MiB), cancellation and the original 300-second command deadline.
No hidden retry, hedging, cross-origin pagination or broker operation is allowed.
Dropping a future does not undo remote work. This crate supplies no task executor,
timer, durable budget ledger or distributed limiter; later adapters and workers
must implement and qualify those obligations.

## Response and retention

Call `validate_batch` after the provider result with current catalogs, clock and
cancellation state. It requires unchanged rights/review versions and rejects
expired deadlines/retention, foreign source/command/instrument/date, empty or
over-4096 observation inventories, future knowledge and licensed data-delay
violations. Rights delays measure completed bar bucket end, not the start label;
cancellation records use their event instant. Provider errors are typed and
payload-free. Only classified transient failures may be retried within remaining
durable budgets; other failures never trigger source-priority guesses.

`KnowledgeMode::RetainedAt` requires reviewed historical-version support and
canonical receipts no later than the requested cutoff. A newly fetched historical
price has current receipt time and cannot satisfy this mode. No live provider
currently has this capability qualified by these synthetic tests.

Canonical retention ends at the earliest admission-check time plus licensed
lifetime, local 24-hour ceiling, rights expiry or explicit deletion deadline.
Raw retention defaults to no permission: requests must explicitly choose retain
and the grant must permit it with a positive lifetime. Its deadline cannot exceed
canonical retention. Keep these exact deadlines; response validation never
extends them. Later use requires another current-rights check; a token is not a
durable read authorization. Storage must separately implement physical deletion,
immediate revocation denial, encryption and orphan cleanup.

Response conformance is not whole-series quality. Apply
[daily quality](market-data.md#daily-series-quality-acceptance) to retained
references/calendar/actions and the exact provider-day schedule before publishing.
The acquisition request remains explicit-range-only: latest-session discovery,
universe admission, storage and runtime coordination remain later M03 slices.
The [evidence bridge](evidence-artifacts.md) now rechecks this authority and daily
quality to seal immutable wire artifacts; it performs no publication or I/O.
No provider or model calls occur in default verification.

## Verification

Run `cargo xtask verify --check market-data-conformance`, then `cargo xtask verify`.
The six named proofs use invented canonical observations, review revisions and a
synthetic port implementation. Reuse `validate_batch` in later adapter suites,
adding actual parser/HTTP and cancellation/deadline tests there. A declaration,
passing synthetic provider or OSS library license cannot qualify real data rights.
