# M08 durable-execution decision proofs

## TL;DR

These isolated Rust packages are decision evidence, not production workspace
members. From the repository root, run
`cargo xtask verify m08-<package>` for one candidate's format, lint, and test
contract; `cargo xtask checks` lists its commands and prerequisites. Tests
start their own temporary database, broker, or local workflow server; no
service URL, credential, CLI installation, or manually started container is
required. Container-backed tests require a running Docker-compatible daemon.
Cold dependencies and container images require network access; the Temporal and
Flawless tests fetch digest-checked server binaries on each run.

## Common synthetic scenario

The point-in-time M08 input is fixed, synthetic, and contains no market-data
provider or model request. Step A simulates preparation; Step B stands in for a
long-running model call. Tests stop a worker or server process during B, then
restart against the same durable state. Duroxide and the event-spine negative
control use a parent-initiated kill; Temporal and DBOS workers abort; Flawless
restarts its server while a synthetic HTTP effect is pending. Passing native
activity recovery requires Step A's invocation count to remain one and B to
complete after restart. Flawless instead checks that its recorded A effect does
not repeat; deterministic local code can replay. A completed Step B is not
evidence that a real external call is exactly once: an external effect can
finish just before the worker dies and before its result is durably recorded.
Production activities
must use provider idempotency keys or reconciliation where supported.

| Package | Command | What it proves |
| --- | --- | --- |
| `event-spine-proof` | `cargo xtask verify m08-event-spine-proof` | Negative control using the real inbox and JetStream adapters: Step A commits and is acknowledged, but Step B has no durable continuation after a worker crash. |
| `durable-execution` | `cargo xtask verify m08-durable-execution` | Duroxide file-backed SQLite history, child-process termination, and resumed Step B without rerunning Step A. |
| `temporal-proof` | `cargo xtask verify m08-temporal-proof` | Temporal Rust SDK local server, child-process termination, and persisted workflow recovery; the harness downloads and SHA-256-verifies a pinned CLI archive. |
| `dbos-proof` | `cargo xtask verify m08-dbos-proof` | Two distinct proofs: PostgreSQL app-data plus SQL enqueue rollback/commit, and native Rust SDK Step A/B crash recovery. The latter does not prove a Rust worker can consume the former's SQL-enqueued row. |
| `flawless-proof` | `cargo xtask verify m08-flawless-proof` | Digest-pinned beta.3 server process crash/restart: previously recorded A HTTP effect once, safely retried B, and terminal C effect once. Pure local A code is not proven to be skipped. |

The [M08 CI matrix](../.github/workflows/m08-durable-execution.yml) uses the same
executable inventory. It runs `m08-<package>-static` for formatting and Clippy,
then the pinned dependency-policy action, then `m08-<package>-test` for the
automated tests on Linux. This preserves the advisory/license gate before
the expensive runtime proof. The full local `m08-<package>` profile does not
substitute for that separate advisory check. Run `cargo xtask verify
dependency-audit` for current locked policy across the root and all five proof
workspaces, or select the proof's named `m08-<label>-dependency-policy` check
from `cargo xtask checks`. The pinned `cargo-deny` tool and network are required
for those advisory checks. Daily maintenance and scheduled audits cover these
independent resolutions; see the [supply-chain guide](../docs/development/supply-chain.md#independent-workspaces-and-dependency-maintenance).
Missing prerequisites report
not run, not a passing crash-recovery proof. Its [experimental license policy](deny.toml)
permits four additional permissive licenses for these proofs; it
does not change the production workspace policy. The exact assertions,
environment limitations, and decision
consequences are recorded in [ADR-0013](../docs/decisions/0013-select-durable-workflow-execution.md).

Each proof has its own `Cargo.lock`. A feature change in a root workspace crate
used through a path dependency can invalidate a proof lock even when its
`Cargo.toml` is unchanged. Refresh only the affected proof with
`cargo metadata --offline --manifest-path experiments/<package>/Cargo.toml
--format-version 1`, review the lockfile diff, then rerun its `--locked` CI
commands. The root workspace's `make verify` does not resolve these separate
experiment locks.

Flawless's server binary is downloadable but closed source. The automated
proof verifies a locally pinned content digest before execution. Its replay
re-executes deterministic code from the beginning and skips recorded effects,
so the A HTTP-effect assertion does not satisfy a literal guarantee about pure
local Step A code. The ADR treats these as different guarantees.
