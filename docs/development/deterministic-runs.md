# Deterministic research and simulation runs

## TL;DR

- P05 supplies finite offline runs in `crates/simulation-domain`.
- Freeze historical P04 readiness, never current evidence relabeled with old time.
- Verify retained artifacts and bind code/configuration in the manifest.
- Commit domain state, outputs and checkpoint atomically; fence concurrent owners.

## Workflow

1. Retain versioned canonical observation artifacts and historical continuity/
   readiness. Rights/calendar/source qualification remain mandatory.
2. Freeze each `ReadyEvidence` plan using `FrozenInput::from_ready` and matching
   artifact SHA-256 digests. Availability includes readiness/recovery delays.
3. Construct `RunManifest` with start time, implementation/configuration artifacts,
   explicit seed and at most 4096 descriptors (before deduplication). Configuration
   pins the required input set, universe, strategy, policy/reference/environment
   revisions and numerical assumptions. No silent pagination or eviction.
4. Retain canonical manifest bytes, checked descriptors and artifacts.
   `Run::next_input` advances time to availability; only then load, verify and
   decode that observation's payload. Planning metadata is not future-data permission.
5. Use the single owner's `next_id`/`random_u64`. Commit outputs, domain state and
   `checkpoint()` together; discard uncommitted state after cancellation/crash.

## Recovery and rejection

Rebuild from retained checked descriptors and compare original canonical bytes or
digest. `Run::restore` skips committed inputs and continues clock/IDs/RNG. Do not
recompute availability from today's provider response or replace missing artifacts.
A content hash binds bytes, not entitlement, completeness or semantic correctness.

Changed code/config/seed/evidence/order defines a different run.
Feed/epoch bindings cannot silently switch source, transformation or semantic/
continuity policy revisions; such a change needs a new epoch.
Invalid checkpoint size/version/checksum/manifest/cursor/time fails closed. Counter exhaustion and
invalid clock movement leave state unchanged. Virtual time cannot pass pending
inputs. Future timers must define an explicit pinned same-time ordering policy.

Checksums do not authenticate storage. Valid stale/concurrent checkpoints can
repeat identities/effects: storage authorization, rollback protection, fencing and
atomic output/state/checkpoint commits belong to the application adapter.

## Verification and boundaries

Run `cargo test --locked -p edgeagent-simulation-domain` and `cargo xtask verify`.
Workspace CI discovers offline unit/integration/doctests covering reorder/duplicate
inputs, restart at each prefix, readiness availability, same-time ordering, clock
bounds, mismatched manifests, corrupt/truncated checkpoints, resource/counter limits
and independent ChaCha20 vectors. No provider, model, broker, database or container.

No persisted repository, payload decoder, orders, fills, risk or timers in P05.
P07/P09/P13 add consumers; M04 is not complete. See
[ADR-0033](../decisions/0033-freeze-deterministic-run-continuations.md) and the
[expansion plan](plans/simulation-expansion.md).
