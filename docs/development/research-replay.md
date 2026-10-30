# Read-only Research replay

## TL;DR

- `offline-replay` recalculates an existing completion and compares exact original bytes.
- Verify the original input/inbox, completion and outbox before owner evidence reads.
- Pin compiled code names and the existing reviewed implementation/configuration profile.
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

The active owner profile must match request implementation/configuration digests.
Those digests remain the existing trusted operator-reviewed mapping to executable
and policy artifacts, including max-age and expiry-horizon settings. This command
does not measure executable hashes, recover historical binaries or invent a new
configuration format. Use the original reviewed profile; do not relabel changed
code/configuration with an old digest. Explicit algorithm pins and byte comparison
add checks but do not replace that provenance authority. Expiry is recalculated
from original decision/horizon/retained rights, never current wall time.

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
owner executable double; actual MarketData read authorization has its separate
existing owner-read gate. Default acceptance protects both test inventories.

Checked SQL metadata is generated against a disposable schema and compiled offline;
the canonical SQLx gate remains required when its pinned CLI is available. Report
unavailable tools as not run. Rollback stops replay processes and restores the prior
binary; no replay-owned persisted state or migrations need undoing. General
backtesting, parameter search and complete four-process qualification remain out of
scope; the latter belongs to M04-PR15.
