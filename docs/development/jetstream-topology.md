# Registry-derived JetStream topology

## TL;DR

- `edgeagent_messaging_nats::topology` owns deployment-time planning and explicit
  provisioning; runtime publishers and consumers never provision themselves.
- Build a `TopologyPlan` from a validated registry, explicit command-publisher
  and event-subscriber bindings, an environment namespace, and bounded limits.
- Install separate credential-free permission objects on distinct identities,
  then provision through the control-plane identity with runtime intake quiesced.
- Existing drift fails closed. Never purge, delete, or recreate retained resources
  to make a conflict disappear.
- `cargo xtask verify --check nats-topology` owns authenticated isolated broker
  proofs. Docker is the default; an explicit pinned native executable is supported.

## Ownership and plan

The [M02 completion plan](plans/m02-completion.md) sequences this capability after
the synthetic payload contracts and before process-worker composition.
[ADR-0025](../decisions/0025-derive-bounded-jetstream-topology-from-the-registry.md)
owns its durable decisions. Registry `owner` means command handler or event
producer, not an arbitrary publisher/subscriber. Explicit bindings fill those
missing authorization inputs; wrong-kind, duplicate, and unregistered bindings
fail before I/O. Event subscriptions are independent durable cursors.

For the synthetic registry, grant Gateway command publication and AuditProjector
fact consumption. Research's command consumer and event publication follow from
its registry ownership. This does not install product handlers or a live runtime.

```rust
use edgeagent_contracts::Component;
use edgeagent_event_spine_conformance::{definitions, registry};
use edgeagent_messaging_nats::topology::{Binding, TopologyLimits, TopologyPlan};

let topology = TopologyPlan::new(
    registry()?,
    "m02",
    &[Binding::new(Component::Gateway, definitions()[0].message_type())],
    &[Binding::new(Component::AuditProjector, definitions()[1].message_type())],
    TopologyLimits::default(),
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The namespace bounds resource names and private reply inboxes only. It does not
rewrite exact published subjects or create account isolation. Two environments
using the same subjects need separate accounts or brokers. Registries are bounded
to 128 definitions and each binding set to 640 entries; derived resource names
must fit 200 bytes. Definition order does not change the sorted plan.

## Broker policy

| Concern | Current profile |
|:--|:--|
| Stream ownership | One file-backed stream per exact registry subject |
| Commands | WorkQueue retention; one owning handler durable shared by replicas |
| Events | Limits retention; independent subscriber durables; ACK does not erase facts |
| Age | Registry command/workflow/audit windows: 7/30/365 days |
| Capacity | Default 10,000 messages and 64 MiB per stream; DiscardNew |
| Consumer count | One command handler; up to five declared event subscribers |
| Envelope | Portable 256 KiB body; broker 257 KiB including a 1 KiB transport-header reserve |
| Deduplication | Two-minute broker window; never replaces durable inbox identity |
| Admission | Default 32 outstanding ACKs and 16 waiting pulls per durable |
| Requests | One-message batches; maximum 30-second pull expiry |
| Recovery | Explicit ACK; 120-second ACK wait; unlimited broker redelivery until age expiry |
| Storage/replicas | File-backed stream/consumer state; one replica; no HA claim |

`TopologyLimits::new` permits bounded environment overrides; it rejects zero,
negative/unlimited, and adjacent overflow values. Republish, direct reads,
rollups, mirrors/sources, TTL overrides, async persistence mode, and other optional
capabilities remain disabled. Known server informational metadata and the omitted
default persistence-mode representation are normalized; other pinned SDK fields
participate in exact drift checks. SDK server-version features are independent,
so the adapter explicitly enables all fields needed to check this profile.

NATS includes headers in `max_msg_size`. The transport reserve permits a maximal
portable envelope plus the publisher's deduplication header; the proof covers
512-byte source and 128-byte ID boundaries. It does not widen the raw body or
quarantine limit: the publisher rejects 256 KiB + 1 before I/O, and raw intake
still halts on oversized bodies even if they fit the broker's header reserve.
Per-stream byte capacity must be at least 257 KiB.

DiscardNew preserves accepted evidence at capacity but does not extend registry
expiry or guarantee availability. The pinned server returns generic store error
10077 for full streams. The adapter deliberately keeps `ConfirmationUnknown` for
that generic response; the proof inspects actual broker state to establish that
its excess message was not retained. Callers must retry identical identity/bytes
under the existing recovery-aware budget, not invent a new ID or assume a terminal
payload rejection. Resolve capacity pressure before expiry/retry exhaustion.

Unlimited broker redelivery is not unbounded application retries: durable retry
budgets and committed quarantine still govern terminal disposition. No broker
delivery limit or raw retained message counts as application quarantine. A
protocol-halted adapter needs operator investigation and replacement, not blind
restarts. Partition metadata does not create a global/FIFO ordering guarantee.

## Provisioning and failure recovery

1. Build and review the immutable plan and role permission objects.
2. Configure connection/request timeouts and an overall provisioning deadline.
3. Stop intake and establish a single provisioning writer. Install identities
   and ACLs through the deployment control plane.
4. Call `provision(&control_context, &topology).await` explicitly. It reads all
   existing planned resources and inventories consumers before issuing writes.
5. On exact compatibility, create only missing resources, using strict consumer
   creation, and verify returned configurations. Repeating the same plan succeeds
   without modifying existing state.
6. Start runtime adapters using fresh configured handles and their own identities.

`Conflict` means review the plan against server configuration, including optional
capabilities and undeclared consumers. Stop before any automatic mutation. A
necessary operator repair or retained-state migration requires separate explicit
authorization. This library never updates, purges, deletes, or removes consumers.

`Unavailable` retains the real SDK cause for explicit diagnosis. Public formatting
redacts it; do not log raw source chains. Partial failure or cancellation can leave
confirmed creates present or an unknown create outcome. Inspect and retry the
same plan. Do not compensate by deleting resources. Preflight is not a transaction
or concurrency fence; a concurrent operator can invalidate it.

Rollback stops new intake/provisioning and reverts the tooling or desired-state
version while retaining streams and durables. Do not delete evidence as rollback.
The API makes no hardware power-loss, backup/restore, clustered, cross-account,
custom JetStream-domain, or cross-cloud failover guarantee.

## Identity and deployment handoff

Each `SubjectPermissions` value exposes its component, publish/subscribe allowlists,
required SDK `inbox_prefix`, and a credential-free NATS `permissions` object from
`nats_config()`. Supply the prefix through `ConnectOptions::custom_inbox_prefix`.
Using the default shared `_INBOX.>` would correctly fail the generated ACL.

Runtime allowlists grant exact bound message publication, own stream/consumer
inspection, own pull requests, scoped ACK publication, and only private reply
subscriptions. They omit topology mutation, consumer listing, raw stream reads,
foreign pulls, foreign inboxes, and direct data-subject subscriptions. No broad
response grant or default fallback permission is generated.

The trusted provisioner can address only planned create/info/list API subjects;
it cannot publish messages or pull/ACK deliveries. Subject ACLs cannot constrain
JSON action/configuration contents on create endpoints: the implementation uses
strict create, but a provisioning credential still represents trusted control-plane
authority. Never combine these allowlists with broader inherited/default grants.
ACLs authorize subjects, not CloudEvents source truth or payload validity.

[Prerequisite from M10] Only these resource/permission boundaries enter M02.
`edge-agent-gitops` owns environment desired state, credentials, secret references,
TLS/workload identity, and Argo application installation. `k8s-infrastructure`
continues to own shared substrate. This PR changes neither repository and does
not deploy a broker or production identity.

## Verification

The mandatory offline `cargo xtask verify` runs topology unit tests and discovers
the complete protected ignored suite. It does not execute broker proofs.

```text
cargo xtask verify --check nats-topology
```

The T3 check is part of the existing `local-platform` profile/job. It starts its
own pinned `nats:2.14.7-alpine` container with generated authenticated fixture
permissions, random loopback port, bounded resources, and disposable file storage.
It requires no service URL, external credentials, or manually started database.
The fixture verifies that the image version matches the checked-in local profile.

For constrained local development, `EDGEAGENT_NATS_SERVER` may name an existing
absolute native executable path. Invalid explicit paths fail closed without a
Docker fallback; the fixture requires the exact 2.14.7 version. Release downloads
must be verified against the official release checksum before execution. The
test itself never downloads tools or contacts model providers.

The native and Docker paths run the same protected Rust scenarios and install
the generated role ACLs on an isolated broker. Cases have bounded readiness and
execution deadlines; normal errors, panic, and timeout paths await owned cleanup.
Native children also have kill-on-drop protection. Abrupt process/machine abort
can bypass cleanup; disposable CI environments remain the final boundary, and
operators may inspect leftover containers labelled `edgeagent.proof=topology`.
Never delete a resource merely because its name resembles a fixture.

Evidence includes drift-before-write, retained bytes, repeat provisioning,
undeclared consumers, production adapter publication/consumption, redelivery,
capacity and pending admission, work-queue removal, independent event replay,
and explicit server permission-violation events. Protected discovery mutations
reject missing/renamed/duplicate/extra tests. These proofs do not establish
application transactions, process-worker restart, ACK-loss recovery, production
identity/TLS, HA, or load/performance objectives; those remain later slices.
