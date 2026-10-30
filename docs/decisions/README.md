# Architecture decision records

## TL;DR

- Use an ADR for durable decisions affecting compatibility, evidence meaning, security, data, topology, ownership, or foundational technology.
- Number ADRs sequentially and never reuse a published number.
- Accepted ADRs are historical records; supersede rather than rewrite them.
- Record alternatives, consequences, migration, security, operations, and validation.
- The Rust workspace, portable event architecture, dry-run execution boundary,
  persistence-neutral inbound coordination, and executable dependency policy
  are accepted foundations; repository governance is progressively enforced.
- Outbox completion is fenced by a per-claim generation, including same-owner
  reacquisition after lease expiry.

## When an ADR is required

Create an ADR when a decision materially affects one or more of:

- public contracts or compatibility;
- persistence, durability, evidence meaning, lineage, or migration strategy;
- security, privacy, authorization, or trust boundaries;
- repository, service, or deployment topology;
- language, framework, database, queue, workflow engine, market-data provider,
  model-provider boundary, or build foundation;
- research-artifact lifecycle or cross-component behavior; or
- a constraint that future contributors might otherwise simplify away.

Local implementation choices that are cheap to reverse do not require an ADR.

## Naming and lifecycle

Use a four-digit sequence and short kebab-case name:

```text
0001-select-the-initial-runtime.md
```

Statuses are Proposed, Accepted, Rejected, Deprecated, or Superseded. Milestone
metadata uses the exact identifier and title published in the roadmap index.
Only an Accepted decision can establish that it supersedes earlier decisions.
When accepted, update each earlier record to Superseded and record both
directions in the same change. A later-superseded replacement retains its own
`Supersedes` history. Proposed, Rejected, and Deprecated records do not name
decisions they supersede. Do not edit the decision or consequences of an
accepted ADR to make history appear cleaner; add a dated note or a new ADR.

Before allocating a number, inspect this index and the decision directory from
fresh repository state. Use the lowest unused number. Resolve a concurrent
collision by renumbering the later unpublished ADR.

The repository-policy checker records each published ADR's number, canonical
filename, and canonical title in its `PUBLISHED_ADRS` identity ledger. Accepted
identity is immutable: do not reuse a number, rename its file, or rewrite its
title. When publishing the next ADR, append its identity to that ledger in the
same change. Removing, renumbering, renaming, or substituting published history
must fail verification.

The `## Index` section uses one Markdown table. Its first rendered row is the
`ADR | Status | Decision` header and every published decision is a body row in
that same table; splitting history across tables is invalid.

## Template

```markdown
# ADR-NNNN: Decision title

- Status: Proposed
- Date: YYYY-MM-DD
- Milestone: MNN - Outcome
- Deciders:
- Supersedes:
- Superseded by:

## TL;DR

Summarize the decision, primary reason, and material boundary.

## Context

What forces a decision now? Include constraints and quality attributes.

## Decision

State the decision and scope precisely.

## Alternatives considered

### Alternative

- Benefits:
- Costs and risks:
- Reason not selected:

## Consequences

### Positive

### Negative

### Neutral or follow-up

## Compatibility and migration

## Security and operations

## Validation

How will the assumptions and consequences be verified?
```

## Index

| ADR | Status | Decision |
|---|---|---|
| [ADR-0001](0001-use-a-rust-workspace-with-multiple-deployables.md) | Accepted | Use a Rust workspace monorepo with independently deployable components. |
| [ADR-0002](0002-use-portable-at-least-once-event-messaging.md) | Accepted | Use versioned CloudEvents contracts and portable at-least-once messaging. |
| [ADR-0003](0003-add-dry-run-trade-execution.md) | Superseded | Add dry-run order execution without broker connectivity or live trading. |
| [ADR-0004](0004-separate-application-ui-gitops-and-substrate-ownership.md) | Accepted | Separate application source, UI, GitOps desired state, and substrate ownership. |
| [ADR-0005](0005-standardize-message-routing-and-retention.md) | Accepted | Standardize message subjects, ownership, partitions, size, and retention. |
| [ADR-0006](0006-keep-inbound-coordination-persistence-neutral.md) | Accepted | Keep inbound orchestration behind a semantic atomic store port while adapters own transactions. |
| [ADR-0007](0007-enforce-workspace-capability-dependencies.md) | Accepted | Enforce package roles and exact direct workspace dependency policy. |
| [ADR-0008](0008-enforce-progressive-coding-harness-policy.md) | Accepted | Enforce progressive repository guidance and deterministic governance policy. |
| [ADR-0009](0009-fence-outbox-transitions-by-claim-generation.md) | Accepted | Fence outbox transitions by claim generation. |
| [ADR-0010](0010-keep-delivery-metadata-transport-neutral.md) | Accepted | Keep delivery metadata transport-neutral. |
| [ADR-0011](0011-bound-untrusted-envelopes-before-decoding.md) | Accepted | Bound untrusted envelopes before decoding. |
| [ADR-0012](0012-keep-message-contract-diagnostics-payload-safe.md) | Accepted | Keep message contract diagnostics payload-safe. |
| [ADR-0013](0013-select-durable-workflow-execution.md) | Accepted | Select Temporal for durable workflow execution. |
| [ADR-0014](0014-use-sqlx-for-postgresql-inbox.md) | Accepted | Use SQLx for the PostgreSQL inbox adapter. |
| [ADR-0015](0015-allow-atomic-sqlx-outbox-enqueue.md) | Superseded | Allow atomic SQLx outbox enqueue. |
| [ADR-0016](0016-use-sqlx-throughout-the-postgresql-outbox.md) | Accepted | Use SQLx throughout the PostgreSQL outbox. |
| [ADR-0017](0017-align-delivery-and-quarantine-bounds.md) | Accepted | Align delivery and quarantine bounds. |
| [ADR-0018](0018-use-validated-message-failure-codes.md) | Accepted | Use validated message failure codes. |
| [ADR-0019](0019-validate-outbox-timing-at-the-port.md) | Superseded | Validate outbox timing at the port. |
| [ADR-0020](0020-retain-settlement-abandonment-diagnostics.md) | Accepted | Retain settlement abandonment diagnostics. |
| [ADR-0021](0021-enforce-ci-security-and-secret-scanning.md) | Accepted | Enforce CI security and secret scanning. |
| [ADR-0022](0022-bound-relay-publication-by-acquired-claims.md) | Accepted | Bound relay publication by acquired claims. |
| [ADR-0023](0023-type-relay-configuration-and-runtime-failures.md) | Accepted | Type relay configuration and runtime failures. |
| [ADR-0024](0024-define-synthetic-event-spine-acceptance-contracts.md) | Accepted | Define synthetic event-spine acceptance contracts and local schema compatibility gates. |
| [ADR-0025](0025-derive-bounded-jetstream-topology-from-the-registry.md) | Accepted | Derive bounded JetStream topology from the registry. |
| [ADR-0026](0026-compose-durable-acceptance-in-independent-processes.md) | Accepted | Compose durable acceptance in independent processes. |
| [ADR-0027](0027-enforce-service-owned-semantic-command-ordering.md) | Accepted | Enforce service-owned semantic command ordering. |
| [ADR-0028](0028-compose-authorized-recoverable-quarantine-replay.md) | Accepted | Compose authorized recoverable quarantine replay. |
| [ADR-0029](0029-export-bounded-correlated-event-spine-observations.md) | Accepted | Export bounded correlated event-spine observations. |
| [ADR-0030](0030-qualify-event-spine-failure-recovery.md) | Accepted | Qualify event-spine failure recovery. |
| [ADR-0031](0031-define-the-supported-market-evidence-profile.md) | Superseded | Define the supported market evidence profile. |
| [ADR-0032](0032-define-vendor-neutral-simulation-profiles.md) | Accepted | Define vendor-neutral simulation profiles. |
| [ADR-0033](0033-freeze-deterministic-run-continuations.md) | Accepted | Freeze deterministic run continuations. |
| [ADR-0034](0034-enforce-versioned-simulated-account-risk-policy.md) | Accepted | Enforce versioned simulated-account risk policy. |
| [ADR-0035](0035-apply-dry-run-order-lifecycles-from-retained-facts.md) | Accepted | Apply dry-run order lifecycles from retained facts. |
| [ADR-0036](0036-serialize-synthetic-account-admission-and-reservations.md) | Accepted | Serialize synthetic account admission and reservations. |
| [ADR-0037](0037-commit-evidence-driven-simulation-execution.md) | Accepted | Commit evidence-driven simulation execution. |
| [ADR-0038](0038-bind-synthetic-cash-settlement-to-explicit-policy.md) | Accepted | Bind synthetic cash settlement to explicit policy. |
| [ADR-0039](0039-commit-account-settlement-as-a-canonical-postlude.md) | Accepted | Commit account settlement as a canonical postlude. |
| [ADR-0040](0040-qualify-bounded-european-option-analytics.md) | Accepted | Qualify bounded European option analytics. |
| [ADR-0041](0041-retain-owner-authored-account-audit-revisions.md) | Accepted | Retain owner-authored account audit revisions. |
| [ADR-0042](0042-freeze-complete-synthetic-sensitivity-evaluations.md) | Accepted | Freeze complete synthetic sensitivity evaluations. |
| [ADR-0043](0043-accept-complete-point-in-time-daily-series.md) | Accepted | Accept complete point-in-time daily series. |
| [ADR-0044](0044-gate-acquisition-on-reviewed-rights-and-provider-authority.md) | Accepted | Gate acquisition on reviewed rights and provider authority. |
| [ADR-0045](0045-freeze-versioned-evidence-artifacts-and-terminal-contracts.md) | Accepted | Freeze versioned evidence artifacts and terminal contracts. |
| [ADR-0046](0046-bound-provider-http-and-preserve-exact-json-tokens.md) | Accepted | Bound provider HTTP and preserve exact JSON tokens. |
| [ADR-0047](0047-store-scoped-evidence-with-create-only-integrity-checks.md) | Accepted | Store scoped evidence with create-only integrity checks. |
| [ADR-0048](0048-persist-acquisition-admission-and-call-budgets.md) | Accepted | Persist acquisition admission and call budgets. |
| [ADR-0049](0049-commit-terminal-acquisition-publication-atomically.md) | Accepted | Commit terminal acquisition publication atomically. |
| [ADR-0050](0050-isolate-invented-daily-sources-and-reuse-frozen-replay.md) | Accepted | Isolate invented daily sources and reuse frozen replay. |
| [ADR-0051](0051-bind-provider-page-dispatch-to-durable-permits.md) | Accepted | Bind provider page dispatch to durable permits. |
| [ADR-0052](0052-freeze-instrument-selection-and-route-headers-before-payloads.md) | Accepted | Freeze instrument selection and route headers before payloads. |
| [ADR-0053](0053-retire-evidence-with-opaque-denials-and-quiescent-cleanup.md) | Accepted | Retire evidence with opaque denials and quiescent cleanup. |
| [ADR-0054](0054-compose-an-offline-only-acquisition-worker.md) | Accepted | Compose an offline-only acquisition worker. |
| [ADR-0055](0055-reserve-reviewed-daily-credits-with-account-permits.md) | Accepted | Reserve reviewed daily credits with account permits. |
| [ADR-0056](0056-preserve-native-dates-with-reviewed-bucket-provenance.md) | Accepted | Preserve native dates with reviewed bucket provenance. |
| [ADR-0057](0057-capture-native-daily-provider-responses-under-durable-permits.md) | Superseded | Capture native daily provider responses under durable permits. |
| [ADR-0058](0058-preserve-alpaca-source-instants-and-bound-historical-sip-pages.md) | Accepted | Preserve Alpaca source instants and bound historical SIP pages. |
| [ADR-0059](0059-accept-native-daily-evidence-with-explicit-owner-lineage.md) | Accepted | Accept native daily evidence with explicit owner lineage. |
| [ADR-0060](0060-persist-native-preparations-and-publish-through-shared-terminal-fences.md) | Accepted | Persist native preparations and publish through shared terminal fences. |
| [ADR-0061](0061-separate-owner-reviewed-qualification-preflight-from-execution.md) | Accepted | Separate owner-reviewed qualification preflight from execution. |
| [ADR-0062](0062-require-independent-postgresql-owner-grants-for-qualification.md) | Accepted | Require independent PostgreSQL owner grants for qualification. |
| [ADR-0063](0063-compose-owner-authorized-finite-vendor-qualification.md) | Accepted | Compose owner-authorized finite vendor qualification. |
| [ADR-0064](0064-bound-streaming-sessions-with-durable-account-fences.md) | Accepted | Bound streaming sessions with durable account fences. |
| [ADR-0065](0065-separate-stream-capture-segments-from-evidence-readiness.md) | Accepted | Separate stream capture segments from evidence readiness. |
| [ADR-0066](0066-publish-stream-capture-through-durable-lifecycle-fences.md) | Accepted | Publish stream capture through durable lifecycle fences. |
| [ADR-0067](0067-bound-wss-frames-before-provider-decoding.md) | Accepted | Bound WSS frames before provider decoding. |
| [ADR-0068](0068-qualify-iex-streams-with-exact-offline-subscriptions.md) | Accepted | Qualify IEX streams with exact offline subscriptions. |
| [ADR-0069](0069-bound-offline-stream-supervision-with-unresolved-heads.md) | Accepted | Bound offline stream supervision with unresolved heads. |
| [ADR-0070](0070-fence-stream-restores-with-durable-incident-barriers.md) | Accepted | Fence stream restores with durable incident barriers. |
| [ADR-0071](0071-pin-iex-canonical-interpretation-to-complete-owner-profiles.md) | Accepted | Pin IEX canonical interpretation to complete owner profiles. |
| [ADR-0072](0072-authenticate-stream-owner-inputs-and-reserve-namespaces.md) | Accepted | Authenticate stream owner inputs and reserve namespaces. |
| [ADR-0073](0073-bind-native-stream-companions-to-publication-receipts.md) | Accepted | Bind native stream companions to publication receipts. |
| [ADR-0074](0074-compose-finite-offline-streams-behind-durable-restore-fences.md) | Accepted | Compose finite offline streams behind durable restore fences. |
| [ADR-0075](0075-support-linux-development-hosts.md) | Accepted | Support Linux development hosts. |
| [ADR-0076](0076-version-deterministic-research-contracts.md) | Accepted | Version deterministic research contracts. |
| [ADR-0077](0077-version-bounded-offline-history-recipes.md) | Accepted | Version bounded offline history recipes. |
| [ADR-0078](0078-validate-frozen-research-evidence.md) | Accepted | Validate frozen research evidence. |
| [ADR-0079](0079-version-raw-price-momentum.md) | Accepted | Version raw-price momentum. |
| [ADR-0080](0080-render-ranked-research-outcomes.md) | Accepted | Render ranked research outcomes. |
| [ADR-0081](0081-commit-research-completions-atomically.md) | Accepted | Commit research completions atomically. |
| [ADR-0082](0082-admit-research-through-a-durable-local-gateway.md) | Accepted | Admit research through a durable local Gateway. |
| [ADR-0083](0083-correlate-owner-approved-research-evidence.md) | Accepted | Correlate owner-approved research evidence. |
| [ADR-0084](0084-run-receipt-first-research-workers.md) | Accepted | Run receipt-first Research workers. |
| [ADR-0085](0085-project-immutable-research-outcomes.md) | Accepted | Project immutable Research outcomes. |
| [ADR-0086](0086-persist-and-serve-scoped-research-projections.md) | Accepted | Persist and serve scoped Research projections. |
| [ADR-0087](0087-query-research-through-the-local-gateway.md) | Accepted | Query Research through the local Gateway. |
| [ADR-0088](0088-append-and-project-owner-artifact-lifecycle.md) | Accepted | Append and project owner artifact lifecycle. |
| [ADR-0089](0089-verify-retained-research-with-read-only-replay.md) | Accepted | Verify retained Research with read-only replay. |
| [ADR-0090](0090-qualify-four-process-research-and-history-shortfalls.md) | Accepted | Qualify four-process Research and history shortfalls. |
