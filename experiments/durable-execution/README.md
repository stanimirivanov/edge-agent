# Duroxide M08 crash-recovery proof

Run `cargo test --locked` in this directory. The nested Cargo workspace keeps
experimental dependencies out of EdgeAgent's production dependency graph. The
test starts its own file-backed SQLite store in a temporary directory and does
not require a pre-started service, credentials, a CLI, or Docker. Cargo needs
network access on the first run to download locked crates.

The synthetic workflow schedules Step A (local work), then Step B (a simulated
long-running model call). The test starts a worker subprocess, waits for Step B
to report that it is in flight, abruptly kills the subprocess, and starts a
new worker on the same history store. It asserts the durable instance completes,
Step A's side effect ran once, and Step B entered twice. The barrier avoids a
timing-only crash injection.

The restarted worker also registers an incompatible v2 workflow. The original
instance must remain on v1; its final v1 output proves replay did not silently
migrate it. A duplicate start submitted before a worker creates the instance is
accepted into Duroxide's queue, so the test asserts the stronger downstream
property: the conflicting submission does not replace the original input or
execute Step A twice. A separate test cancels an in-flight Step B and verifies a
terminal cancellation result without completing Step B.

This proves checkpoint replay for a **completed** Step A. It does not claim
exactly-once execution of an interrupted external call: Step B is retried, and
a real model request would require a stable idempotency key or reconciliation
policy when the provider may have accepted the first request.
