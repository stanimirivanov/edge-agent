# ADR-0025: Derive bounded JetStream topology from the registry

- Status: Accepted
- Date: 2026-10-16
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Derive exact-subject file streams and durable pull consumers from the validated
  registry, with explicit command-publisher and event-subscriber grants.
- Keep provisioning outside runtime adapters. Preflight existing resources,
  create missing resources, and reject drift without updates or deletion.
- Bound storage and admission, reject new publications at capacity, and preserve
  registry retention semantics. Broker delivery exhaustion is not quarantine.
- Generate distinct control-plane and runtime subject ACLs with private reply
  inboxes. Installing identities/credentials remains a deployment responsibility.

## Context

ADR-0005 makes the registry authoritative for subjects, owners, and retention.
ADR-0024 supplies synthetic typed acceptance messages. Handwritten broker setup
can still choose incompatible storage, retention, pending limits, or permissions.
M02 needs a reproducible topology boundary before worker-process composition,
without letting a publisher or handler change its own durability policy.

## Decision

Add a deployment-only `topology` module to the NATS infrastructure adapter.
`TopologyPlan` consumes a validated registry and bounded profile. One exact
message subject owns one file-backed stream. Commands use WorkQueue retention
and one durable for their declared handler; events use Limits retention and one
durable per explicitly declared subscriber. Independent event ACKs do not remove
the retained fact. Command handlers and event publishers derive from ownership;
command publishers and event subscribers require explicit validated bindings.

Names derive injectively from validated subject tokens and an environment
namespace. Sorting makes declaration/binding order irrelevant. Published subjects
never acquire environment or aggregate identifiers. Resource-name namespaces do
not isolate identical subjects: environments need separate NATS accounts/brokers.
The current profile uses the default JetStream API domain and one replica. It
does not qualify clustered, cross-account, or custom-domain operation.

Streams use the registry's seven-day command, thirty-day workflow, or 365-day
audit age, a 256 KiB envelope-body limit, explicit finite byte/message/consumer bounds,
and DiscardNew. Capacity rejects a new publication instead of evicting accepted
evidence before its replay window. Registry expiry still applies; this is not an
infinite evidence store. Rollups, per-message TTL, purge, message deletion,
republish, mirrors, sources, direct access, and other optional capabilities stay
disabled. The full pinned SDK configuration participates in drift checks.

NATS counts headers in its stream message-size limit. Reserve 1 KiB above the
portable body ceiling for the publisher's bounded source/id deduplication header.
The proof publishes an exact 256 KiB envelope with a 512-byte source and 128-byte
ID and rejects the adjacent body byte before transport. The broker rejects raw
messages exceeding 257 KiB; raw intake still rejects bodies above 256 KiB even
when they fit the header reserve. This layered limit does not widen the portable
envelope or durable quarantine contract. Disable compression explicitly and
normalize only omitted Default persistence mode plus the server's three known
informational metadata keys; Async and unclassified configuration drift fail.

At capacity the pinned broker wraps rejection in generic store code 10077.
The adapter preserves its conservative ConfirmationUnknown category for that
code, while the proof independently asserts the excess message was not retained.
Retry identical identity/bytes under the recovery-aware budget; do not infer a
terminal payload rejection or create a replacement message identity.

Durables use explicit ACK, full bodies, file-backed state, two-minute ACK wait,
one-message batches, thirty-second maximum pull expiry, and bounded waiting and
unacknowledged requests. Broker redelivery is unlimited (`max_deliver = -1`),
subject to stream age. Application retry budgets and durable quarantine remain
authoritative. A protocol-halted worker requires investigation, not endless
automatic restart. Expiry/ACK wait, resource saturation, and worker readiness
must become monitored operations in subsequent slices.

The explicit `provision` function reads every existing planned stream and its
consumer inventory before writing. Incompatibility or undeclared consumers fail
closed. Only a typed not-found response permits creation. Consumer creation uses
the SDK's strict create action, never its create-or-update convenience method.
It rechecks every returned configuration. No update, purge, delete, or compensating
rollback exists. A quiesced single provisioning writer is required; preflight is
not a distributed transaction or lock. Cancellation, timeout, or partial failure
requires inspection and retry of the identical plan. Previously confirmed
creates remain, allowing restart-safe convergence without deleting evidence.

Generate credential-free NATS permission objects for separate principals.
Runtime users may publish only their bound message subjects, inspect their own
stream/consumer, pull only their own durable, and publish scoped ACKs. They may
subscribe only to their role-specific reply inbox. They receive no topology
mutation, raw message read, consumer listing, foreign pull, or direct data-topic
subscription grant. Callers must set the SDK custom inbox prefix accordingly.
Provisioner grants name only planned create/info/list API subjects, not message
publication or consumption. Subject ACLs cannot inspect a JSON create action or
configuration payload; the provisioning principal remains trusted control-plane
authority, and only this implementation promises strict create behavior. Grants
must not be combined with broader inherited/default permissions.

[Prerequisite from M10] This slice includes only necessary broker resource and
authorization contracts. GitOps owns deployment identities, credential issuance,
TLS/workload identity, secret references, and installing desired state. There is
no runtime auto-provisioning, cloud deployment change, or GitOps repository edit.

The adapter explicitly enables SDK server_2_10/2_11/2_12 fields in addition to the
existing server_2_14 feature: these features are independent, not cumulative.
This is required to inspect disabled optional capabilities and use strict create.
No new production library dependency is introduced. Conformance gains test-only
messaging/NATS composition and Tokio process support, with its locked errno and
signal-hook-registry dependencies (MIT/Apache-2.0 compatible). The existing
maintained, pinned Tokio lifecycle API replaces handwritten process polling;
the fixture module is the replacement boundary. Normal service dependencies are
unchanged. Dependency policy and isolated proof locks remain separately checked.

## Alternatives considered

### Let runtime publishers and handlers call get-or-create

Convenient startup would require control-plane credentials and can hide drift
or create/update consumers. Reject it: runtime adapters continue to accept
preconfigured handles and never call provisioning.

### Share one stream across every message type

Fewer resources would mix command/workflow/audit age, retention, ownership, and
consumer bounds. Exact-subject streams make these contracts directly inspectable;
the bounded registry limits total resources.

### Evict old messages or stop redelivery after a broker delivery budget

These controls can discard recovery evidence or strand work without application
quarantine. Reject new publications at capacity; application-owned durable retry
and quarantine still determine terminal disposition.

## Consequences

The plan provides an immediately executable provisioning/ACL boundary and
authenticatable acceptance fixture without premature product services. Strict
configuration checks can reject innocuous future SDK/server changes; review
their exact semantics rather than ignoring arbitrary fields. Default validation
remains offline. T3 proofs own an isolated authenticated server, bounded process
lifecycle, public fixture credentials, loopback ports, and cleanup after results,
panic, or deadline. They use the pinned Docker image by default or an explicitly
selected pinned native binary for constrained local development.

## Compatibility and migration

Existing subjects, payloads, production adapter entry points, SQL, and retained
state remain unchanged. Do not apply this plan over existing ad-hoc test streams
or active environments without comparing configuration and quiescing operators.
Conflicts require an explicit migration/new profile or authorized repair, not
automatic update/recreation. Rollout installs reviewed identities and plan with
runtime intake stopped, verifies broker state, then starts authorized runtimes.
Rollback stops new intake/provisioning and reverts tooling/configuration while
retaining streams and consumers; deleting them is never a rollback prerequisite.

## Security and operations

ACLs prove subject authority, not that arbitrary clients supplied truthful
CloudEvents source fields or valid payloads. Existing typed publisher/registry
validation remains necessary. Public provisioning errors expose bounded categories
only; SDK sources remain available for explicit diagnosis and must not be logged
unredacted. Callers must configure connection/request timeouts and an overall
provisioning deadline. Hardware power-loss recovery, HA, production identity/TLS,
backups, complete operational alerts, and cross-cloud failover are not proved.

The NATS documentation describes [retention and capacity](https://docs.nats.io/nats-concepts/jetstream/streams),
[durable consumer limits](https://docs.nats.io/nats-concepts/jetstream/consumers),
and [subject authorization](https://docs.nats.io/learn/security/authorization).
The pinned SDK source determines concrete configuration/API behavior.

## Validation

Offline adapter tests cover deterministic derivation, limits, invalid binding
sets, ownership/retention, least-privilege grants, full configuration mutation,
and payload-safe error/source behavior. The protected T3 `nats-topology` suite
uses production publisher/consumer adapters and synthetic M02 payloads against
an exclusive authenticated server. It tests repeat application, preflight drift
before writes, preserved retained bytes, undeclared consumers, allowed/denied
subjects, work-queue ACK removal, independent event replay, redelivery, capacity,
payload rejection, and pending admission. Explicit broker permission events prove
denial; a timeout alone never does. The default ignored-test census and T3
discovery reject missing/renamed/duplicate/extra proofs. Process-worker recovery,
ACK-loss qualification, and application idempotency remain later M02 slices.
