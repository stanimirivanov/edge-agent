# NATS adapter refactor execution plan

## TL;DR

- Milestone: **M02 - Contracts and event spine**.
- Preserve application-owned ports, immutable publication identity, exact
  bounded evidence, and confirmation-before-success semantics.
- Deliver three slices: broker-boundary correctness, stopped consumer lifecycle,
  then cohesive modules and independently verified conformance fixtures.
- All three slices implement admission, confirmation, stopped lifecycle, and
  cohesive receive ownership. Discovery is not evidence that a broker proof ran.
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

The ownership state replaces the Boolean gate: only `Active` can retain an SDK
stream. Protocol faults drop that resource synchronously. Exhaustion and the
SDK's terminal deleted/push-consumer errors enter `Closed`; all four nonterminal
error kinds preserve active intake. Slice 3 narrows activated stream ownership
to the receive future, so cancellation releases that worker without closing
logical intake. T2 drop probes protect each path;
two registered T3 proofs keep handles alive across expiry/reconnect and broker
consumer deletion. Merely adding Fuse would not stop the owned task. No finite
`max_deliver` requirement is introduced and no disposition occurs on release.

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

The implementation preserves the public facade and gives publication, intake,
pure metadata derivation, and settlement cohesive private owners with adjacent
tests. Consumer diagnostics expose state only. Shared broker fixtures retain
failure evidence, bound cleanup, and separate synthetic subjects.

The independent intake proof found that batch size one did not prevent the SDK
from renewing while a caller held a delivery. Each polled receive now owns and
releases its activated stream; later calls create a fresh stream. Cancellation
retains configured intake but can require ack-wait redelivery. The new proof
uses permissive broker limits, observes the renewal window, and drains the
remaining retained work. A separate proof verifies cleanup on all failure paths.
Neither proof claims a global pending limit or a hard buffer bound during
uncertain network recovery.

## Compatibility, rollout, and rollback

Public signatures, crate-root exports, envelope bytes, delivery identity,
portable error categories, and SQL schemas remain unchanged. Header-only
configuration now fails admission; provision full-body consumers before rollout
and recreate handles after quiesced configuration changes. The relay already
quarantines explicit rejection instead of automatically spending retry budget.
Unknown outcomes preserve immutable retry and inbox deduplication policy.

Rollback restores unsafe acceptance/classification behavior and is not a
recommended recovery mechanism. Correct provisioning or isolate the affected
service rather than treating unreadable replies as success. Resource lifetime
changes preserve message evidence and public paths. Cancelling a receive can
wait for broker redelivery; review ack-wait and retry budgets before rollout.
Rollback restores background renewal while intake is idle and is not recommended.

## Verification and evidence limits

Run `cargo xtask verify` for full default acceptance. The existing ignored-test
census compiles and protects all named proofs without starting a broker. Run
the thirteen NATS checks through `cargo xtask verify --check ID` against an exclusive
local server; the [messaging guide](../messaging-adapters.md#verification) names
the new boundary checks. Existing local-platform CI runs them with the pinned
Compose server and always destroys that isolated environment afterward.

T2 pure tests do not establish broker behavior. They reject invalid status and
nonempty settlement bodies. Real-stream tests prove publication rejection,
positive confirmed settlement, redelivery, and no-responder outcomes after
consumer deletion. Lifecycle proofs independently observe pull cessation after
the SDK renewal deadline and reconnect, and immediate closed-state returns.
The intake-bound proof observes idle receive behavior with permissive broker
limits; it does not retract issued requests or prove a hard buffer bound during
network recovery. Missing services or tools must be reported as not run, never passed.

## Separate follow-ups

Service readiness, bounded reconstruction, compatible handler/delivery budgets,
and advisory capture belong to runtime/provisioning. Any bounded oversized-
evidence disposition requires a separate ADR and evidence schema. Reusing the
encoded envelope between validation and publication is a cross-crate
optimization, not permission to bypass size validation in this refactor.
