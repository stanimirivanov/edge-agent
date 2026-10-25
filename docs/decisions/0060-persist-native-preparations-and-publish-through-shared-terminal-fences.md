# ADR-0060: Persist native preparations and publish through shared terminal fences

- Status: Accepted
- Date: 2026-10-25
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Persist one exact native acceptance before PUT; retry its original bytes and identity.
- Share scoped create-only storage, denial, deletion and terminal transactions.
- Typed readers reject other formats; raw capture is never accepted evidence.
- Separate native correction/observation lanes from precise source-time lanes.
- Keep provider activation and authenticated vendor runtime qualification blocked.

## Context

[ADR-0059](0059-accept-native-daily-evidence-with-explicit-owner-lineage.md) seals
quality-checked native dates without fabricating source instants. Crashes between
acceptance, object confirmation and terminal COMMIT must not allocate new versions,
advance acceptance time or publish another outcome. Retention must cover newly
durable licensed preparation bytes, not only object-store data.

## Decision

Add explicit application-owned native storage and retention operations. Reuse the
same SDK create/read/collect helpers and exact descriptor/key contract. The existing
`evidence/daily-v1` physical prefix is an opaque location convention, not a precise
payload discriminator. Canonical bytes explicitly select accepted native or precise
decoding. Typed readers reject the other format; both reject raw native captures.

The reference-only terminal event remains unchanged: it promises accepted evidence,
not precise source availability. Consumers must inspect supported format under
authenticated retrieval; unsupported formats fail closed. Upgrade consumers before
native activation; never convert dates into fabricated instants for old readers.

Persist one canonical native preparation/digest per admission in the same transaction
as pre-PUT inventory. Exact retries compare retained bytes; different preparations
conflict. Recover before acquisition or identity allocation. Revalidation uses the
original acceptance time and exact frozen inputs while current rights, cancellation,
deadline and access remain current checks. Uncertain COMMIT/PUT requires recovery,
never compensating deletion or replacement identities.

Read predecessors through retained scoped storage before accepting corrections.
Use native-specific scoped series/observation identities. Fingerprints bind raw rows,
full temporal provenance and predecessor revision, excluding derived mapping/actions
and transformation. Reuse head compare-and-swap and immutable-version guards in the
single receipt/inbox/outbox transaction. ACK only confirmed COMMIT or recovery.

## Alternatives considered

- Duplicate stacks: diverging authorization and failure rules.
- Convert dates into source instants: false precision and incompatible evidence meaning.
- Regenerate preparation after restart: changes acceptance, lineage and digest.
- Publish full evidence on the bus: expands licensed copies and retention obligations.
- Add a format field to the existing descriptor: breaks its strict published schema.

## Consequences

Native publication is a library capability, not activated vendor composition.
Preparation adds at most 4 MiB per native command; deployment admission quotas and
cleanup remain owner responsibilities. Authenticate source/reference inputs and local
authority independently. Failed confirmation can leave an inventoried orphan without
a receipt; the existing retirement path handles it.

## Compatibility and migration

Apply private migration 0004 after 0001–0003 before native preparation APIs. It creates
an empty additive table with an admission foreign key and cascading erasure. Existing
precise/capture schemas, physical keys, terminal routes and goldens remain unchanged.
Stop native intake before rollback; retain the table and use an erasure-aware binary
for maintenance. Do not reactivate a binary that ignores licensed native preparations.

## Security and operations

Retirement erases preparation bytes with admission, retaining the existing opaque
denial and deletion inventory until quiescent physical cleanup. Revocation denies
recovery/retained reads at the local durable boundary. SQL deletion does not scrub
WAL/media/backups. Cloud erasure and broker copies still need qualification.
Diagnostics redact payloads. All verification uses invented offline data; no vendor,
credential, model or live broker operation is enabled.

## Validation

Protect model/storage fault tests and isolated PostgreSQL proofs with independent
named-test censuses and mutation tests. Prove uncertain PUT/restart identity, terminal
rollback, ACK-loss recovery, competing corrections, denial and preparation erasure.
Regenerate checked SQLx metadata against the complete isolated schema; run the full
default harness. Adapter/client recovery is not OS process-death or cloud qualification.
