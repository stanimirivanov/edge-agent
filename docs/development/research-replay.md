# Read-only Research replay

## TL;DR

- `offline-replay` recalculates an existing completion and compares exact original bytes.
- Verify the original input/inbox, completion and outbox before owner evidence reads.
- Validated replay uses the retained configuration and compares the full policy decision.
- Never publish, add inbox receipts, bypass owner rights or turn read failure into success.

## Activation and retained inputs

Use the [Research worker](research-worker.md) loopback database, private schema,
operator scope and owner profile settings. Add two explicit pins:

| Variable | Required compiled revision |
|---|---|
| `EDGEAGENT_RESEARCH_REPLAY_RENDERER` | `research-ranking-raw-v1` |
| `EDGEAGENT_RESEARCH_REPLAY_MOMENTUM` | `raw-usd-roc-ppm-half-even-v1` |

Invoke `edgeagent-research offline-replay` with the original incoming canonical
MarketData evidence-decided envelope on closed stdin (at most 262144 bytes).
The envelope must already be the original committed completion's input, with an
intact `research-completion` inbox receipt, matching source/ID/bytes, causation and
correlation. A later duplicate with a new delivery ID is not the original input.
Changed intent, unknown completion, missing/corrupt receipt or damaged outbox proof
is refused. Retain the original envelope with its completion; there is no arbitrary
expected-output file or automatic export/discovery command in this slice.

Successful stdout is the exact recalculated canonical original artifact/abstention
record. Repeated independent processes produce identical bytes and no publication.
Failure uses the existing nonzero safe CLI diagnostic and produces no result bytes.
The operation has a 25-second deadline; owner child reads retain their 15-second,
four-MiB limits, kill/reap behavior and current authorization checks. Descriptor
commands and existing worker/relay behavior remain unchanged.

## Provenance and comparison

The [application verifier](../../crates/research-application/src/replay.rs) accepts
no store, commit, inbox, outbox or broker port. It checks the explicit renderer and
momentum names against compiled constants and binds the retained result's request
and publication causation/correlation to the original input. Ready input always
reloads current owner-authorized retained bytes, verifies their original digest/
descriptor and converts them through the existing frozen-evidence boundary. It then
reruns deterministic eligibility, momentum, ranking and rendering and requires
exact byte equality with the independently retained owner completion.

For a validated completion, the read-only lookup returns the same retained
configuration it checked against the decision, historical approval and control
snapshots. The CLI uses that configuration's implementation, age and expiry
settings for the complete owner policy evidence read. The active worker
configuration file and its policy environment settings are unnecessary. The
operator still supplies the owner connection, scope and compiled revision pins.

The selected validator recomputes eligibility, liquidity, volatility, gap, event,
regime and evidence-quality policy, as well as the terminal result. Replay
requires exact canonical equality for both the terminal bytes and the decision,
including its eligibility set and reason codes. Stdout remains the original
terminal record. Expiry is recalculated from decision time, horizon and retained
rights. For receipts predating decision companions, use the legacy active owner
profile settings and close-only verifier. See
[ADR-0106](../decisions/0106-replay-retained-research-policy-decisions.md).

Input ordering is not output identity: the pure evidence boundary orders closes
and the renderer orders candidates/lineage. Outer envelope JSON formatting may
vary; retained inbox comparison uses the existing canonical envelope encoding.
An incompatible code pin, changed active revision, wrong binding, corrupt evidence,
calculation refusal or unequal bytes is not successful replay.

A retained owner-declared abstention can reproduce its stable reason without an
evidence read. Legacy direct EVALUATE requests deterministically reproduce their
missing-entitlement policy outcome. For Ready input, denied/unavailable/invalid/
unsupported reads refuse replay: they must not be converted into a new abstention
or used to certify a historical read-failure abstention. Evidence erasure, current
retention expiry and revoked rights can legitimately make replay unavailable.
No current-data substitution, historical permission bypass or lifecycle revival
occurs. Reproducing bytes never establishes current artifact validity/entitlement.

## Persistence, verification and rollback

The [owner lookup](../../crates/research-postgres/src/lib.rs) runs a strictly
read-only transaction. Unlike normal receipt recovery, it never registers a new
inbox delivery. It validates only Research-owned tables and closes the transaction
before external owner I/O. Replay cannot create a second completion/publication or
change lifecycle rows. No schema migration or external dependency is added.

Run `cargo xtask verify --check research-replay --python python3` for protected
credential-free recalculation/permutation, revision/intent/output refusal,
read-failure and owner-declared abstention tests with map-order feature unification.
With an isolated loopback `EDGEAGENT_POSTGRES_URL`, run
`cargo xtask verify --check research-replay-postgres --python python3` for actual
independent replay processes, revision/input/corruption/read refusals and full
completion/lifecycle/inbox/outbox row snapshots. The process proof uses an invented
owner executable double, including a reason drift case with the same terminal
abstention. Actual MarketData read authorization has its separate
existing owner-read gate. Default acceptance protects both test inventories.

Rollback stops replay processes and restores the prior binary. The schema and
checked query statements are shared with normal Research receipt recovery.
