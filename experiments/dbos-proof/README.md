# DBOS decision-gate proofs

This standalone Rust package runs two separate DBOS proofs against disposable,
pre-migrated [DBOS-maintained PostgreSQL test images][dbos-image] through
`testcontainers-rs`. No local database, migration CLI, credentials, or manual
container startup is required; an available Docker-compatible daemon is the
testcontainers prerequisite.

The SQL atomicity test checks one narrow capability: a Rust SQL client
can insert service-owned data and invoke the official `dbos.enqueue_workflow`
PostgreSQL function in the **same transaction**.

Run from this directory:

```text
cargo test --locked
```

The test first forces a unique-key failure after enqueue, then checks outside
the rolled-back transaction that neither the application row nor the DBOS
workflow row survived. It repeats the operation and commits, checking that
both rows are visible. The test uses the same database (`dbos_test_1`) and one
PostgreSQL transaction for each pair of writes. The committed workflow is
`ENQUEUED` on `m08_evaluations` and records `portable_json` serialization.

The native SDK test registers a synthetic two-step workflow with DBOS Rust SDK
0.5.0. A child worker completes and checkpoints Step A. It then increments a
Step B attempt counter and aborts the process, bypassing graceful
shutdown. The harness launches a new child with the same workflow registration
and application version. It asserts the recovered result, `SUCCESS` status,
Step A count of one, Step B count of two, and the SDK's `rust_serde`
serialization marker. This proves checkpoint replay
for an already-completed step after process death; the in-flight Step B is
repeated. The test does **not** establish exactly-once behavior for an external
effect interrupted between that effect and its DBOS checkpoint.

The SQL atomicity test **does not** prove that the current DBOS Rust SDK can
execute a workflow enqueued by this SQL function. The function writes
`portable_json` inputs;
[Rust SDK issue #45][rust-serializer] records that its portable serializer is
not implemented. The native recovery test starts a workflow through the Rust
SDK instead, so it does not bridge that compatibility gap. Neither test proves
transactional workflow steps or exactly-once external model calls:
[Rust SDK issue #48][rust-steps] records the absence of transactional steps.
Those are separate decision gates.

The image reference is pinned to an OCI index digest, not a mutable tag. Its
schema is migration 108, so the proof deliberately checks only the documented
workflow-status row, not later payload tables. If the image becomes
unavailable, replace it with another verified official DBOS schema image and
record its digest before rerunning the proof.

[dbos-image]: https://github.com/dbos-inc/dbos-ctl#prebaked-test-database-images
[rust-serializer]: https://github.com/dbos-inc/dbos-transact-rust/issues/45
[rust-steps]: https://github.com/dbos-inc/dbos-transact-rust/issues/48
