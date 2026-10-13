# NATS adapter refactor execution plan

## TL;DR

- Milestone: **M02 - Contracts and event spine**.
- Preserve application-owned ports, immutable publication identity, exact
  bounded evidence, and confirmation-before-success semantics.
- Deliver three slices: broker-boundary correctness, stopped consumer lifecycle,
  then cohesive modules and independently verified conformance fixtures.
- Slice 1 implements admission and confirmation changes; slices 2 and 3 remain
  outstanding. Compilation/discovery is not evidence that a broker proof ran.
- No live trading, model call, workflow framework, wire-format change, database
  migration, or new dependency belongs in these slices.

## Outcome and authority

The adapter must reject incompatible configuration, classify publication
failures by evidence, and return settlement success only for a valid broker
confirmation. Application code continues to depend on the portable messaging
ports, not NATS types. The [messaging guide](../messaging-adapters.md) owns
operational behavior and [the harness](../harness.md) owns acceptance routing.

[ADR-0017](../../decisions/0017-align-delivery-and-quarantine-bounds.md) requires
oversized raw deliveries to remain unsettled while the consumer halts for
investigation. [ADR-0020](../../decisions/0020-retain-settlement-abandonment-diagnostics.md)
requires payload-safe abandonment diagnostics and forbids settlement from Drop.
Neither decision permits treating maximum-delivery exhaustion as quarantine.

## Ordered slices

### 1. Broker admission and confirmation correctness

- Reject header-only consumer configuration before starting a pull stream.
- Preserve exhaustive phase-aware SDK-kind classification. Inspect typed causes
  for known rejection codes; do not match free-form server descriptions.
- Classify typed message-size rejection (10054) as `Rejected`; retain unknown
  confirmation for undecodable, generic storage, and unclassified server errors.
- Use SDK request/reply for ACK, delayed NAK, and TERM, preserving the configured
  timeout. Require a successful empty response; no-responder and malformed
  replies never establish confirmation.
- Protect full base-kind and source-sensitive mapping, oversize pre-I/O
  rejection, configuration admission, and response redaction with T2 tests.
- Enroll five named T3 proofs in the canonical inventory and NATS task routes.

The SDK exposes message context, client, timeout, and acknowledgement encoding,
so this correction needs no public constructor change or vendored SDK fork.

### 2. Stop the owned consumer worker on terminal transitions

- Represent active, protocol-halted, and closed intake states explicitly.
- Release the SDK stream on protocol failure and proven terminal closure,
  stopping its background task even while the adapter handle remains alive.
- Preserve genuinely transient errors rather than closing on every failure.
- Prove cessation across expiry/reconnect and document already-issued request
  uncertainty. Dropping a worker does not revoke broker-side effects.

The current Boolean gate blocks application polling but retains the SDK stream.
The SDK task can issue pulls on expiry/reconnect; merely adding Fuse does not
stop it. This remains an explicit outstanding slice, not a claimed guarantee
from slice 1's gate tests. No finite `max_deliver` requirement is introduced.

### 3. Cohesive modules and isolated conformance

- Keep the crate-root facade while separating publisher and consumer ownership.
- Extract pure metadata derivation, adjacent test modules, and safe consumer
  Debug. Keep pull, validation, construction, and settlement order visible.
- Consolidate shared fixtures with bounded, owned cleanup and separate subjects
  or exclusive broker/account ownership. Do not pre-delete shared resources.
- Independently prove the documented intake bound without masking it with
  `max_ack_pending = 1` and `max_batch = 1` on the broker.
- Distinguish request batch size, client buffering, and consumer-global pending
  acknowledgements. A brief pending-count snapshot is not a sustained bound.
- Extend existing public API documentation and deterministic harness checks.

File length is a review signal, never an extraction acceptance criterion.

## Compatibility, rollout, and rollback

Public signatures, crate-root exports, envelope bytes, delivery identity,
portable error categories, and SQL schemas remain unchanged. Header-only
configuration now fails admission; provision full-body consumers before rollout
and recreate handles after quiesced configuration changes. The relay already
quarantines explicit rejection instead of automatically spending retry budget.
Unknown outcomes preserve immutable retry and inbox deduplication policy.

Rollback restores unsafe acceptance/classification behavior and is not a
recommended recovery mechanism. Correct provisioning or isolate the affected
service rather than treating unreadable replies as success. Slice 2 changes
resource lifetime, not message evidence; slice 3 preserves public paths.

## Verification and evidence limits

Run `cargo xtask verify` for full default acceptance. The existing ignored-test
census compiles and protects all named proofs without starting a broker. Run
the nine NATS checks through `cargo xtask verify --check ID` against an exclusive
local server; the [messaging guide](../messaging-adapters.md#verification) names
the new boundary checks. Existing local-platform CI runs them with the pinned
Compose server and always destroys that isolated environment afterward.

T2 pure tests do not establish broker behavior. They reject invalid status and
nonempty settlement bodies. Real-stream tests prove publication rejection,
positive confirmed settlement, redelivery, and no-responder outcomes after
consumer deletion. Missing
services or tools must be reported as not run, never passed.

## Separate follow-ups

Service readiness, bounded reconstruction, compatible handler/delivery budgets,
and advisory capture belong to runtime/provisioning. Any bounded oversized-
evidence disposition requires a separate ADR and evidence schema. Reusing the
encoded envelope between validation and publication is a cross-crate
optimization, not permission to bypass size validation in this refactor.
