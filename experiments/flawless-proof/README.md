# Flawless M08 effect-boundary proof

## TL;DR

- Run `cargo test --locked` in this directory. The test downloads the pinned
  Flawless `1.0.0-beta.3` server, verifies its SHA-256 before execution,
  automatically provisions the Rust Wasm target if absent, builds the synthetic
  workflow, and uses a temporary server data directory and local HTTP mock.
- The test kills the server process during a pending synthetic B request,
  restarts it, and asserts that recorded A and C effects happen once while the
  explicitly retry-safe B effect completes after retry.
- This is **not** proof that pure local Step A code is skipped. Flawless
  intentionally re-executes deterministic code during replay. A is deliberately
  an HTTP effect boundary in this experiment; no real model is contacted.

## Scope and result

The test is isolated from the production Cargo workspace and services. It
requires the ordinary Rust/rustup toolchain and network access to crates.io,
Rust target distribution, and the official Flawless server download endpoint.
No environment variables, external CLI installation, database startup, or
manual server startup are required. It does not contact a model provider.

The official closed-source server binary is fetched directly from
`https://downloads.flawless.dev/1.0.0-beta.3/<platform>/flawless[.exe]`.
The checked-in digests were observed from that HTTPS endpoint during this
evaluation; Flawless did not publish a vendor-signed digest or checksum sidecar
at the probed artifact locations. The test fails closed if the downloaded
binary differs from the pinned digest. It does not redistribute the binary.

Artifact provisioning uses a separate 10-second connect timeout and 120-second
whole-response deadline, independent of the 20-second localhost control client.
It permits at most three fresh GET attempts for transport/body failures, HTTP 429
or 5xx, with one- and two-second backoffs (at most 363 seconds of download budgets).
Each attempt streams at most 512 MiB into a private partial file and recomputes
SHA-256 from the start. Only a completely verified file becomes executable;
partial files are removed on failure. Other HTTP errors, oversized artifacts,
digest mismatch and local file errors fail immediately. No unverified cache,
changed digest, partial-range resume or workflow-effect retry is introduced.

From the repository root, run the credential-free loopback provisioning regressions
without downloading or executing Flawless:

```text
cargo test --locked --manifest-path experiments/flawless-proof/Cargo.toml tests::download::tests::
```

They cover a truncated response followed by a clean retry, transient HTTP status,
terminal HTTP/hash/size rejection and bounded retry exhaustion. The complete
server/Wasm crash proof remains a separate CI execution.

| Platform | Pinned SHA-256 | Execution result |
| --- | --- | --- |
| Windows x64 | `b06723cce23b6e599f39440f6781b68ea45d6a96a3c0761715fbcb1bd729fccf` | Passed locally, 2026-10-05. |
| Linux x64 | `b511598babcde8a8bc7898f6e0c7b06b3f51a2d26227fba1cc480b06af5a215c` | Not run locally. |

Other platforms are outside this experiment. The test is only compiled on
Linux x64. The Wasm target is installed automatically by
`rustup` if necessary; this changes the contributor's existing Rust toolchain
installation, but requires no manual step.

## What the assertion means

The synthetic workflow sends A, B, then C HTTP effects to a local Rust server.
The B endpoint withholds its first response. The parent test observes B's
arrival, terminates the Flawless process, releases the pending B handler, and
starts a new Flawless process against the same data directory. A completed
recorded effect must not be emitted again; `.idempotent()` on B permits its
uncertain call to be retried. The test checks `A == 1`, `B >= 2`, and `C == 1`.

Flawless [documents](https://flawless.dev/docs/) that replay recomputes
deterministic code from the beginning and reuses logged side-effect results.
Thus a pure local Step A computation may run again; only its recorded effect
is skipped. [Flawless's retry-safety documentation](https://flawless.dev/docs/idempotence/)
also says an interrupted HTTP request can return `RequestInterrupted` unless
the caller explicitly marks it idempotent. A real model-provider adapter would
need its own stable request identity and idempotency policy; this synthetic
test does not prove exactly-once provider invocation.
