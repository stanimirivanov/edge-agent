# Offline daily acquisition worker

## TL;DR

MarketData composes daily acquisition for **invented data only**. No vendor factory,
API key lookup, provider HTTP call or live mode exists. PR11D/PR11E and a separate
activation review remain mandatory. See
[ADR-0054](../decisions/0054-compose-an-offline-only-acquisition-worker.md).

## Explicit local profile

Build: `cargo build --locked -p edgeagent-market-data`. Commands: `offline-init`,
`offline-seed <request-id>`, `offline-worker`, `offline-relay`, and
`offline-maintain --owner-confirmed-quiescence <request-id>...`.
Legacy `describe` and `self-check` remain unchanged.

Set `EDGEAGENT_POSTGRES_URL` to loopback PostgreSQL and `EDGEAGENT_OFFLINE_ROOT`
to a dedicated local directory. `USE_MOCK` must be absent, `1` or `true`;
`EDGEAGENT_MARKET_DATA_PROVIDER` must be absent or `synthetic-daily-v1`.
Other modes/providers fail before infrastructure lookup. No vendor credentials
are consumed. This CLI is trusted local operator authority, not a public API.
Infrastructure URLs reject query parameters/fragments, including host overrides.

`offline-init` requires an empty directory, creates a root-derived private schema
with existing inbox/outbox/acquisition migrations and writes the profile marker.
It never drops or upgrades an existing schema. Partial SQL/filesystem bootstrap
fails closed: inspect the dedicated resources before choosing a fresh root. Never
blindly reinitialize or delete a shared database. An exclusive local file lock is
held through shutdown. It is not a distributed lease or network-filesystem guarantee.

`offline-seed` requires a unique nonzero 32-character lowercase hexadecimal ID.
It admits one invented bar two UTC dates behind admission (always satisfying the
daily eligibility lag) and prints only `AcquireEvidence`
reference JSON. An authorized local test/Gateway publisher wraps the reference
in CloudEvents. Seeding does not impersonate Gateway or publish. Reuse the retained
command for retries, not re-seeding. The calendar/XNYS label is synthetic.

Worker/relay require loopback `EDGEAGENT_NATS_URL` and
`EDGEAGENT_OFFLINE_MAX_MESSAGES` (1–16). Worker additionally requires
`EDGEAGENT_OFFLINE_STREAM` and `EDGEAGENT_OFFLINE_CONSUMER`. Provision registry-derived
acquisition command and evidence-decision subjects beforehand; runtime never alters
topology. Consumer: durable, full bodies, explicit ACK, command-only filter, ACK wait
at least 30 seconds, max deliveries 1–3, max ACK pending 1–32. Adapter checks also apply.

## Commit, recovery and bounds

Private admission-v2 pins selection, actual synthetic bytes, authority/quality policy
and the compiled recipe. Recipe/config changes reject unfinished old jobs rather
than silently reinterpret them. Legacy v1 stays readable but is not executable as
this worker's pinned v2 profile. Terminal recovery precedes source work; a receipt
never grants evidence-read permission.

One acquisition runs at a time: original duration 300 seconds, three attempts,
four pages, 1 MiB response and 8 MiB command ceilings. Synthetic work consumes
**zero external permits/provider quotas**. Current invented authority, whole-series
quality and retained immutable storage precede the existing atomic receipt/inbox/
outbox commit. ACK follows confirmed commit or persisted retirement/revocation denial.
Permanent poison is durably quarantined before broker termination; the finite worker
then stops nonzero. Transient/uncertain work retries after fifteen seconds. Incomplete
preparation can repeat and leave bounded inventoried orphans; no exactly-once external
effect is promised.

Pool: four connections, five-second acquisition/statement timeout and one-second
lock timeout. Creation-only bootstrap allows thirty-second statements, five-second
locks and sixty seconds total for cold DDL. Broker connect/drain: five
seconds; idle receive: thirty; object/relay operations: fifteen. SIGINT/SIGTERM cancels
work with fifteen-second handler grace followed by bounded pool/runtime shutdown.
Phases are cumulative, not a single fifteen-second SLA. No replacement starts during
shutdown. `offline-relay` uses the existing recovery-aware relay; `relay_progress`
reports an iteration, not necessarily successful publication.

## Retention, restore and rollback

Startup performs bounded retirement; completed deliveries run retirement/cleanup
(16 requests per batch).
Confirmed completed writes can authorize their own cleanup. Restart/uncertain writes
require explicit owner confirmation after old writers are joined/fenced. Maintenance
accepts at most sixteen exact request IDs as the operator's assertion. A timeout is
never evidence. No CLI clears unresolved account calls: generation-fenced blocks
remain until separately owner-confirmed quiescence through the existing owner port.

Restore must preserve root, private schema, immutable objects and recipe together.
Missing profile/schema state fails closed, but receipt recovery does not inspect
every retained object or qualify backup completeness. This does not qualify cloud backup erasure,
multi-host HA, cloud restore or live readiness. Older binaries cannot read admission-v2:
stop intake before rollback and retain a compatible binary for outstanding jobs,
or initialize a fresh invented profile. Never rewrite persisted admissions.

## Verification boundary

`cargo xtask verify --check offline-worker-contracts` protects portable mode,
pinning, cancellation and quiescence. `cargo xtask verify --check offline-worker-processes`
uses existing local PostgreSQL/NATS endpoints and real binaries in owned fixtures:
receipt-first restart, one object, relay, revocation and explicit cleanup.

`cargo xtask verify --check offline-worker-recovery` requires loopback
`EDGEAGENT_POSTGRES_URL` and Docker (or pinned `EDGEAGENT_NATS_SERVER`). It owns
each file-backed broker, private schema, temporary object root and child process;
scenarios run serially with bounded deadlines and teardown on failures/panics.
The owned broker is unsecured and binds only loopback, matching the current
trusted-local worker connection factory. These are not broker-authentication proofs;
M02's authenticated permission fixtures remain separate and unchanged.
Missing prerequisites fail, never silently skip. CI discovers this local-platform
gate and protects all three ignored proof identities before execution.

- A PostgreSQL wire gate holds terminal COMMIT before forwarding it. Killing the
  actual worker rolls back receipt/inbox/outbox together; redelivery prepares again
  and commits one transition. Uncommitted preparation can leave inventoried orphans.
- Separate NATS faults withhold an ACK request or only its confirmation. Request
  loss plus worker death and same-store broker restart retains pending delivery;
  receipt-first redelivery skips preparation. Confirmation loss reports uncertainty
  while broker pending is zero; a logical duplicate recovers identical receipt bytes.
  Tests retain the production thirty-second ACK wait, not a shortened test profile.
- Independent ledger processes contend for one account while a second account
  progresses. Forced death preserves pending permits and conservative byte/call
  charges; new external dispatch stays blocked until explicit owner-confirmed quiescence.
  Synthetic work progresses without clearing that pending permit or consuming quota.
  Confirmation never refunds unknown usage; resumed synthetic work consumes zero
  external calls/bytes. No provider is contacted by these quota actors.

Fault interception exists only in integration fixtures, not production switches.
This proves committed transition uniqueness, not exactly-once transport/external
effects, PostgreSQL server crash recovery or distributed writer fencing. M10 retains full
operational readiness, authenticated live authority and restore qualification.

## Versioned multi-session recipes

`offline-seed <request-id> [recipe]` defaults to `single-v1`. Explicit names:

| Recipe | Invented data | Quality result |
| --- | --- | --- |
| `single-v1` | Original single bar, two UTC dates behind admission | Accepted |
| `history-v1` | Six sessions, oldest seven UTC dates behind admission | Accepted |
| `history-missing-v1` | Same six-session range with its third observation omitted | Rejected |
| `history-action-mismatch-v1` | Complete raw series with an unsupported action on its first session | Rejected |

Each history close starts at USD 123.45 and increases by USD 1 per session;
OHLC are equal and volume is zero. These are raw provider-day observations,
not adjusted prices or total returns. The mismatch fixture exercises refusal to
reinterpret unsupported corporate-action history, not a synthetic adjusted-price
claim. Every UTC date is explicitly an invented open session, including weekends:
this calendar is not XNYS qualification or historical constituent membership.
The universe is the single `INVENTED` instrument selected by the retained catalog.

The fixed six-session bound, one-day buckets, explicit session references and
observation versions are part of the recipe. All observations become available
at admission, never at their older event dates. Readiness for history recipes
permits a finite 90-day age and six-day skew; the legacy policy stays unchanged.
No caller-supplied arrays, dates, prices or unbounded session counts are accepted.

Worker recovery selects the recipe by exact retained admission identity, including
configuration; a changed recipe cannot replace unfinished work. Completed receipts
still recover first. The frozen source fixture in
[`offline-single-v1`](../../services/market-data/fixtures/offline-single-v1/)
is the original byte-for-byte implementation used to preserve its historical
configuration binding and independently test legacy compatibility. Do not edit it.
New recipe identities bind their name and current implementation bytes.

No migration, schema upgrade, provider activation or new runtime mode is needed.
Use a new request ID for a different recipe; retain the original command on retry.
The existing `offline-worker-contracts` gate includes recipe replay, legacy
compatibility, changed-configuration refusal and complete/negative quality tests.
