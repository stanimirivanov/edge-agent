# ADR-0005: Standardize message routing and retention

- Status: Accepted
- Date: 2026-09-26
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Derive broker subjects from exact major-version message types instead of
  maintaining adapter-specific routing strings.
- Give every command one owning handler and every event one authoritative
  producer.
- Carry aggregate identity in `partitionkey`, not in broker subjects.
- Cap every portable structured envelope at 256 KiB.
- Use explicit seven-day command, thirty-day workflow-event, and 365-day
  audit-event retention classes.
- Treat the validated Rust message registry as the source for future broker
  configuration, authorization, and conformance evidence.

## Context

ADR-0002 establishes CloudEvents metadata, at-least-once delivery, per-aggregate
ordering, and portable broker profiles. The first envelope contract validates
message identity but intentionally leaves routing, ownership, retention, and
payload size undefined.

If adapters select these values independently, NATS and managed-cloud profiles
can route the same message differently, grant overly broad permissions, expire
facts before recovery completes, or accept payloads that another provider
rejects. Encoding aggregate identifiers into subjects would also create
unbounded subject and authorization cardinality.

## Decision

Every supported major-version message has one `MessageDefinition` containing
its CloudEvents type, immutable schema URI, command/event kind, owner,
partition namespace, and retention class. A validated `MessageRegistry`
rejects invalid and duplicate definitions and performs exact type resolution.

Derive subjects mechanically:

```text
edgeagent.command.<domain>.<name>.v<major>
edgeagent.event.<domain>.<name>.v<major>
```

Commands use work-queue delivery. The declared owner is the only component that
handles the command, although multiple replicas may share its durable consumer
group. Events use retained-stream delivery. The declared owner is their sole
authoritative producer, and its component URI must match the CloudEvents
`source`. Event consumers remain independent and do not acquire ownership by
projecting a fact.

The `partitionkey` uses `<declared-prefix>/<identifier>`. Aggregate identifiers
never enter subjects. Ordering is required only within one exact partition key.

The complete structured envelope, including metadata and `data`, must not
exceed 256 KiB. Larger evidence is stored externally and referenced by an
immutable URI, digest, size, media type, and entitlement metadata.

Retention classes are:

- commands: retained until acknowledged or seven days, whichever comes first;
- workflow events: replayable for at least thirty days; and
- audit events: replayable for at least 365 days.

An adapter may provide greater raw broker capability but cannot weaken or
silently reinterpret these application contracts. Broker configuration,
publish/subscribe authorization, and conformance tests will derive from the
registry rather than copy its values.

## Alternatives considered

### Define subjects and retention inside each broker adapter

- Benefits: adapters can use provider-native conventions directly.
- Costs and risks: routing, authorization, replay, and migration semantics can
  diverge silently across profiles.
- Reason not selected: provider adapters implement one portable application
  contract; they do not own message meaning.

### Put aggregate identifiers in subjects

- Benefits: broker filters can target a single aggregate.
- Costs and risks: unbounded subjects and ACLs, higher operational cardinality,
  and inconsistent mappings on managed brokers.
- Reason not selected: `partitionkey` expresses ordering without turning domain
  identifiers into routing topology.

### Use each provider's maximum payload size

- Benefits: fewer object-storage references on brokers with large limits.
- Costs and risks: messages accepted in one profile may be undeployable in
  another, and retries amplify memory and network pressure.
- Reason not selected: one conservative application limit keeps artifacts
  portable and encourages explicit evidence references.

### Leave retention entirely environment-specific

- Benefits: operators can minimize cost for each deployment.
- Costs and risks: replay, incident recovery, projection rebuild, and audit
  claims have no dependable window.
- Reason not selected: retention is part of observable correctness, not only a
  storage setting.

## Consequences

### Positive

- Subjects, ownership, ACL inputs, size policy, and recovery windows become
  reviewable compatibility contracts.
- New transports can generate configuration and tests from one registry.
- Low-cardinality subjects remain stable as aggregate volume grows.
- Oversized evidence fails before publication and follows the object-reference
  pattern consistently.

### Negative

- Retention windows create predictable storage cost that profiles must expose.
- A 256 KiB ceiling requires claim-check mechanics for larger evidence.
- Changing subject or partition conventions requires a staged compatibility
  migration rather than an adapter-only edit.

### Neutral or follow-up

- Domain message definitions are added only with their typed payload and golden
  fixture; this decision does not reserve speculative message names.
- Per-component publisher and subscriber ACL generation arrives with the NATS
  adapter and concrete process registries.
- Quarantine subjects and retry schedules remain part of consumer-runtime work.

## Compatibility and migration

No broker subjects or durable messages exist, so this decision moves no live
state. Once a subject has been published, compatible payload changes retain its
type and schema major version. Breaking changes add a new versioned type and
subject, deploy tolerant consumers first, and retire the prior route only after
its replay window closes.

A broker-profile migration may bridge old and new subjects only when it
preserves CloudEvents identity and records the original transport metadata.
The same event received through both paths remains one business fact.

## Security and operations

- Command ownership becomes the basis for subscribe ACLs; event ownership
  becomes the basis for publish ACLs.
- Exact subjects and bounded partition metadata limit wildcard and cardinality
  abuse.
- Size checks run before an outbox or transport accepts a message.
- Retention configuration, storage estimates, deletion, and replay permissions
  require profile-level review and conformance evidence.
- Unknown major versions, schema mismatches, wrong event producers, invalid
  partitions, and oversized envelopes fail closed before domain handling.

## Validation

- Unit tests derive stable command and event subjects.
- Registry tests reject duplicate and unknown message types.
- Contract tests reject mismatched schemas, partitions, event sources,
  retention semantics, and oversized envelopes.
- Future broker adapters must derive configuration from the registry and pass
  equivalent delivery, authorization, retention, and replay tests.
