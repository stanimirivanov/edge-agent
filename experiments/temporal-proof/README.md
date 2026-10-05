# Temporal M08 crash-recovery proof

Run `cargo test --locked --manifest-path experiments/temporal-proof/Cargo.toml` from the
repository root. This is an isolated Cargo workspace and does not change the
product workspace or deployable services.

The test starts and stops an ephemeral Temporal CLI dev server through the
official Rust SDK's `testing` feature. Before startup, the test downloads the
official Temporal CLI `v1.8.3` tarball and verifies its SHA-256 against the
release's [published checksums][checksums]. It supports Linux x64 and Windows
x64; another platform requires an explicitly reviewed artifact hash. The
verified executable is passed to the SDK as `EphemeralExe::ExistingPath`, so
the SDK does not perform its own unchecked download. The test needs network
access on each run, but no environment variables, manual CLI installation,
Docker, or separately started database.

The worker runs as a child process. Step A records one invocation and completes;
Step B writes a first-attempt marker and aborts the child. The harness observes
the process failure, starts a new worker, awaits the original workflow, asserts
that A ran exactly once, and replays the completed server history. This tests
resumption after a *recorded* activity completion. It does not establish
exactly-once external effects: a real provider request can be repeated if a
worker crashes after the provider acts but before activity completion is
recorded. Production activities still need provider idempotency keys or an
application-owned effect ledger.

[checksums]: https://github.com/temporalio/cli/releases/download/v1.8.3/checksums.txt
