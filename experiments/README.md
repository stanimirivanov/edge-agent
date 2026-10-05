# M08 durable-execution decision proofs

## TL;DR

These isolated Rust packages are decision evidence, not production workspace
members. Each runs with `cargo test --locked` from its own directory. Tests
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
| `event-spine-proof` | `cargo test --locked` | Negative control using the real inbox and JetStream adapters: Step A commits and is acknowledged, but Step B has no durable continuation after a worker crash. |
| `durable-execution` | `cargo test --locked` | Duroxide file-backed SQLite history, child-process termination, and resumed Step B without rerunning Step A. |
| `temporal-proof` | `cargo test --locked` | Temporal Rust SDK local server, child-process termination, and persisted workflow recovery; the harness downloads and SHA-256-verifies a pinned CLI archive. |
| `dbos-proof` | `cargo test --locked` | Two distinct proofs: PostgreSQL app-data plus SQL enqueue rollback/commit, and native Rust SDK Step A/B crash recovery. The latter does not prove a Rust worker can consume the former's SQL-enqueued row. |
| `flawless-proof` | `cargo test --locked` | Digest-pinned beta.3 server process crash/restart: previously recorded A HTTP effect once, safely retried B, and terminal C effect once. Pure local A code is not proven to be skipped. |

Run a package's command from its listed directory, or use
`cargo test --locked --manifest-path experiments/<package>/Cargo.toml` from the
repository root. The [M08 CI matrix](../.github/workflows/m08-durable-execution.yml)
runs formatting, Clippy, dependency policy, and the automated tests on Linux
for every executable candidate. Its [experimental license policy](deny.toml)
permits four additional permissive licenses for these proofs; it
does not change the production workspace policy. The exact assertions,
environment limitations, and decision
consequences are recorded in [ADR-0013](../docs/decisions/0013-select-durable-workflow-execution.md).

Flawless's server binary is downloadable but closed source. The automated
proof verifies a locally pinned content digest before execution. Its replay
re-executes deterministic code from the beginning and skips recorded effects,
so the A HTTP-effect assertion does not satisfy a literal guarantee about pure
local Step A code. The ADR treats these as different guarantees.
