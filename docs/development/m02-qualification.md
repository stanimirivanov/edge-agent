# M02 failure qualification

## TL;DR

- Run `cargo xtask m02-qualification` with Docker available. No database URL,
  credentials, tool installation or manual service startup is needed.
- The command owns pinned PostgreSQL; fixtures own authenticated pinned NATS,
  worker processes, private schemas and restricted identities.
- Retain `artifacts/m02-qualification/<run-id>/results.tsv` after teardown.
  Failed/unavailable suites never qualify individual scenarios.
- Default `cargo xtask verify` compiles/discovers ignored proofs; it does not
  establish runtime recovery evidence.

## Execution and evidence

The command invokes the existing `message-schema-contracts`, `nats-topology`
and `event-spine-processes` checks with protected discovery. Process fixture
admission stays at two scenarios; internal contenders and deadlines remain.
The `m02-qualification` verification profile exposes infrastructure suites for
explicitly managed environments but does not provision dependencies. Prefer
the canonical command for unattended execution. Legacy `local-platform`
adapter regressions remain separate to avoid repeated suites in CI.

| Fault | Required evidence/recovery |
|---|---|
| Producer killed before commit | No visible acceptance/outbox; fresh producer commits both once. |
| Producer killed after enqueue commit | Visible immutable intent survives; exact retry creates no duplicate. |
| Relay killed after persisted receipt | Leased/unpublished row survives; owned fixture lease expiry permits fresh fenced retry and one broker message/effect. |
| Consumer killed after commit/before ACK | Inbox, state/receipt and fact exist; actual later delivery is a duplicate and confirms ACK. |
| File broker killed/restarted | Same store retains durable identity and pending message; fresh worker handles actual redelivery. |
| ACK request dropped | One observed request, none forwarded; unknown result, pending ACK, fresh worker deduplicates. |
| ACK confirmation dropped | One forwarded request, exact reply dropped; unknown result, independently confirmed empty pending state. |
| Broker deduplication expires | Same identity gets Duplicate then Persisted; inbox still suppresses a second transition. |
| Malformed/unsupported/poison input | Exact bytes quarantined before terminal settlement; no domain writes. |
| Oversized intake | Adapter halts without ACK/quarantine; separate explicit broker size rejection adds no pending message. |

Prior cases cover delayed/reordered input, transient poison exhaustion,
semantic conflict, competing replicas, connection interruption, authorized
fenced replay/fan-out and exported telemetry/failure isolation. Fault fixtures
shorten ACK wait to 200 ms. The deduplication case uses a two-second window and
polls actual receipts, not a sleep to guess progress. Production topology and
timing remain unchanged.

Docker may reassign an ephemeral host port on restart. The fixture refreshes
the exact owned container mapping before readiness and fresh-worker recovery;
the durable broker/store identity, not a host port, defines continuity.

## Lifecycle and limitations

An exclusive run directory/container label scopes PostgreSQL cleanup, including
lost create receipts. Reports survive teardown. Only owned containers/anonymous
volumes, broker stores, schemas and roles are removed. Missing prerequisites,
census drift, test failure and cleanup failure return nonzero. CI uploads
reports even on failure with missing-artifact rejection and 14-day retention.
Partial failed suites report `not-qualified`, not invented per-case successes.

An optional absolute `EDGEAGENT_NATS_SERVER` path selects pinned native 2.14.7
instead of NATS containers; no override is required. Ambient PostgreSQL/NATS
service URLs cannot redirect qualification. Ordinary error/panic/deadline
paths await cleanup; parent abort/runner termination needs disposable-runner
teardown. Never prune shared Docker resources.

These proofs do not qualify PostgreSQL server restart, power-loss/fsync,
multi-node HA, cloud, load, product-service lifecycle or production operations.
No model/provider, local model runtime, market or live trading call runs.
See [ADR-0030](../decisions/0030-qualify-event-spine-failure-recovery.md),
[process acceptance](process-acceptance.md) and the [M02 plan](plans/m02-completion.md).
