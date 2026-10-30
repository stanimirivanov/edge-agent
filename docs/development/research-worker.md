# Offline Research worker

## TL;DR

- Explicit stdin/JetStream worker modes recover Research completion before owner I/O.
- MarketData exports current authorized evidence through its owner-read command.
- Existing deterministic policy/rendering and atomic completion/inbox/outbox are reused.
- Scope, policy revisions, private schema, owner endpoint and resource bounds are explicit.

## Configuration and modes

`edgeagent-research` keeps its descriptor commands. The new single-argument modes are
`offline-once`, `offline-worker` and `offline-relay`. [Read-only replay](research-replay.md)
and [owner lifecycle](research-lifecycle.md) add separate explicit operator commands. Prepare existing Research,
inbox and outbox migrations in an independent Research-owned schema; startup checks
completion-table readiness and never runs DDL. Use a restricted runtime role with
existing completion/inbox/outbox permissions and no completion mutation or DDL.

All modes require `EDGEAGENT_POSTGRES_URL` (loopback PostgreSQL, no URL routing
query/fragment overrides), `EDGEAGENT_RESEARCH_SCHEMA` (a `research_` prefix followed
by lowercase ASCII/digits/underscore, at most 63 bytes), and
`EDGEAGENT_RESEARCH_SCOPE` (1–64 ASCII letters/digits/period/underscore/hyphen).

Stdin/worker modes additionally require:

| Setting | Meaning |
|---|---|
| `EDGEAGENT_RESEARCH_OWNER_BINARY` | Absolute path to the independently installed MarketData binary |
| `EDGEAGENT_RESEARCH_OWNER_ROOT` | Absolute owner profile root; Research never opens its object directory |
| `EDGEAGENT_RESEARCH_OWNER_DATABASE_URL` | Independently configured loopback owner database passed only to the owner process |
| `EDGEAGENT_RESEARCH_OWNER_ACQUISITION` | Operator-selected admitted canonical acquisition ID |
| `EDGEAGENT_RESEARCH_IMPLEMENTATION` | Nonzero lowercase 64-hex implementation revision matching reviewed code |
| `EDGEAGENT_RESEARCH_CONFIGURATION` | Nonzero lowercase 64-hex immutable policy revision |
| `EDGEAGENT_RESEARCH_MAX_AGE_MS` | Positive maximum latest-close age, at most 31 days |
| `EDGEAGENT_RESEARCH_TTL_MS` | Positive expiry horizon after decision, at most 300000 milliseconds |

These values are reviewed operator policy, never claims supplied by a client. This
first profile supports one owner-exported raw USD daily-v1 instrument and an exact
lookback+1 session suffix. Unsupported revisions/universe abstain. Receipt recovery does not require the owner executable
or profile to remain available; path availability is checked only during a new read. Mapping/calendar
identities hash complete retained owner references; close, source receipt, capture
and reference knowledge are preserved. No current calendar, float arithmetic,
provider observation time or model output invents availability or numeric facts.
Expiry is the smaller of decision+horizon and retained rights expiry. Historical
evaluation can return an already expired artifact; consumers must honor its expiry.

`offline-once` reads one canonical envelope from a closed stdin pipe (262144-byte
maximum) and returns exact committed artifact/abstention bytes. It needs no broker.
`offline-worker` and `offline-relay` require loopback `EDGEAGENT_NATS_URL` and
`EDGEAGENT_RESEARCH_MAX_MESSAGES` (1–16). Worker mode also requires explicit
`EDGEAGENT_RESEARCH_STREAM` and `EDGEAGENT_RESEARCH_CONSUMER`. Provision a durable
consumer for the MarketData evidence-decided subject or the legacy Research EVALUATE
subject, with explicit ACK, at least 60-second ACK wait, exactly three maximum
deliveries and 1–32 pending ACKs. Topology is not provisioned automatically.
SIGINT/SIGTERM stop intake; active work has a finite deadline.

## Authorized read and evaluation

The loader calls only the configured MarketData executable with
`offline-research-read <owner-acquisition-id>` and the exact owner publication on
stdin. The isolated child gets only the selected offline scope/profile/database.
MarketData checks its exact retained publication/acquisition and current owner
rights/retention before returning canonical object bytes. Revocation and opaque
retirement denial never return bytes. The read creates no Research-side access to
MarketData tables. Owner-only root/table operations remain inside the owner process.

The child output limit is four MiB plus a one-byte overflow probe, with a fifteen-
second total deadline. Failed/timed-out children are killed and reaped. Raw child
stderr is discarded. Exit 77 means denied/absent; 65 invalid; 75 transient storage/process availability, including a busy owner profile lock.
The adapter independently verifies descriptor/digest/canonical bytes and current
retention expiry. Submillisecond inputs fail instead of flooring availability.
The existing [evidence gate](research-evidence.md) decides point-in-time eligibility;
readability alone does not establish eligibility. No arbitrary fetch locations are
accepted. An owner-declined result preserves its stable reason without reading.
Legacy direct evaluation has no descriptor and drains to missing-entitlement
abstention. No false evidence is substituted for unavailable data.

## Commit, retries and relay

The [application worker](../../crates/research-application/src/worker.rs) calls
`recover` before any loader work. Retained completion verifies exact request,
canonical outcome/event and outbox integrity. Repeated delivery and process/result
loss return first bytes even if the owner has since become unavailable. New work
performs all owner I/O/calculation before the existing
[Research completion transaction](research-persistence.md).

ACK follows commit/recovery. Invalid source/correlation, foreign scope or conflicts
are quarantined durably before broker termination. Unavailable owner reads leave
completion absent and retry after one second. At attempt three, recover once more
to resolve ambiguous completion, then quarantine confirmed absent work with
`research_retry_exhausted`. Storage/settlement failures leave the delivery unresolved.
The bounded relay publishes exact retained outcome envelopes using existing leases,
confirmation and retry/quarantine policy. It never recalculates an artifact.

## Verification and rollback

- `cargo xtask verify --check research-worker-contracts --python python3` checks
  deterministic byte conversion, digest refusal, local endpoint and descriptor behavior.
- With loopback PostgreSQL, run
  `cargo xtask verify --check research-worker-postgres --check research-persistence --check research-evidence-processes --python python3`.
  Research process proofs use an explicit invented owner test double; the separate
  MarketData proof validates actual current owner reads and revoked-read refusal.
- With loopback PostgreSQL/NATS, run
  `cargo xtask verify --check research-worker-broker --python python3` for real ACK,
  duplicate/restart, exact relay, poison correlation and retry exhaustion.
- Run the default gate plus canonical SQLx metadata and supply-chain gates. Missing
  pinned tools are not passing results. No provider or licensed market data is used.

Stop intake/relay before rollback; preserve existing Research state for compatible
readers and retain owner receipts while active. No schema deletion, topology/cloud
operation, provider/model activation or live execution is introduced. Projector,
query and artifact lifecycle changes remain later milestone slices.
