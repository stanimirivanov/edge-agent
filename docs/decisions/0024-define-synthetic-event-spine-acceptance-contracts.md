# ADR-0024: Define synthetic event-spine acceptance contracts

- Status: Accepted
- Date: 2026-10-16
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Define an explicitly synthetic command/fact pair in nondeployable conformance
  tooling, not in production research or trading APIs.
- Derive Draft 2020-12 schemas from the Serde wire types and validate before
  returning private-field typed payloads.
- Select exact local schemas through the existing registry; never retrieve an
  untrusted schema URI over HTTP or from the filesystem.
- Separate artifact freshness, immutable v1 baselines, and conservative optional
  property compatibility evidence. None proves arbitrary schema subsumption.

## Context

The envelope, registry, inbox/outbox, relay, and messaging adapters are already
implemented. M02 still needs a typed acceptance workflow, generated payload
schemas, and compatibility evidence before its multi-process proof. Inventing a
research request or order contract would pull later product behavior into M02.
Adding schema SDKs to the shared production envelope crate would also impose
their footprint on every service despite no current product payload consumer.

## Decision

`edgeagent-event-spine-conformance` is a tooling-role library. Its executable
schema example is used now by `cargo xtask message-schemas`; later M02 slices
will add synthetic process roles. No service depends on this package.

`ApplyProbe` requests a synthetic integer value at an expected aggregate revision;
`ProbeApplied` records the resulting committed revision. The `conformance`
message namespace and `probe` partition prefix distinguish these contracts from
product messages. Existing Gateway/Research component identities label fixture
producer/owner roles only; no production research handler is registered.

Wire fields are private. Constructors and Serde decoding use the same local
schema checks. Envelope decoding first enforces the existing bounded raw parser,
then registry type/schema/producer/partition policy, then payload validation,
then equality of payload aggregate identity and envelope partition. A valid
typed message is not permission to commit: service idempotency, revision checks,
authorization, transactions, and settlement remain later composition concerns.

Schemars 1.2.3 derives explicit Draft 2020-12 schemas from the private Serde wire
models. Jsonschema 0.58.6 validates their values with default features disabled:
no HTTP/file reference resolvers, async resolver, or TLS stack is selected.
Both dependencies are pinned, permissively licensed (MIT for Schemars; MIT for
Jsonschema), and confined to tooling. Their APIs are hidden behind the local
schema/typed-decode boundary, so replacement does not alter production APIs.
The direct feature choices and locked transitive graph remain subject to
architecture review, cargo-deny, and normal maintenance. See the upstream
[Schemars API](https://docs.rs/schemars/1.2.3/schemars/) and
[Jsonschema API](https://docs.rs/jsonschema/0.58.6/jsonschema/).

Generation sorts all object keys recursively and adds exactly one final LF,
independent of serde_json feature unification. The generator writes only the
finite current artifact inventory; it never repairs failures by deleting files
or updating retained baseline snapshots.

The compatibility policy accepts identical schemas or additional optional
top-level properties only. All existing property schemas, required fields, schema
identity, and other root keywords must remain identical; both sides must allow
unknown properties. Unsupported schema shapes and unclassified changes fail
closed. Tests combine structural mutation cases with old/new reader fixtures.
This policy is intentionally not a general JSON Schema language-inclusion solver:
new optional names must not reinterpret previously used extension fields, and
review still owns meaning, default behavior, and rollout.

Published v1 schemas and URIs remain immutable. A passing optional-addition
candidate check does not authorize overwriting them. Unknown optional payload
extensions are already accepted by v1 readers without changing that schema.
Recognizing a new field in a published schema requires a new immutable schema
identity and an explicit registry/rollout decision; mixed schema-revision lookup
is not introduced in this slice. Breaking meaning requires a new message major.

### Dependency license approval

[Prerequisite from M01] The maintainer explicitly approved adding MIT-0 to the
global license allowlist, including future dependencies under that license.
Jsonschema's `referencing` dependency uses `fluent-uri` 0.4.1, which depends on
`borrow-or-share` 0.2.4. Its packaged LICENSE was reviewed against
[MIT No Attribution](https://opensource.org/license/mit-0) and the
[SPDX MIT-0 text](https://spdx.org/licenses/MIT-0.html). It grants permissive use
without an attribution condition. Root `deny.toml` and the independent
supply-chain sensor now approve that SPDX identifier globally; no package
exceptions, advisory ignores, or confidence-threshold reductions are introduced.
These dependencies remain conformance tooling only. Replacing the validator is
the removal path if maintenance or licensing becomes unacceptable; maintainers
own that review under the ordinary dependency-audit policy.

## Alternatives considered

### Implement an actual research or dry-run order payload first

This would couple delivery acceptance to unfinished M04/M07 policy. Synthetic
contracts prove the backbone without claiming a research artifact or order.

### Extend the production envelope crate with schema discovery

This would spread schema tooling to every service and expose untrusted schema
URIs as network/filesystem capabilities. Keep exact local lookup in the consumer
that currently needs it instead.

### Hand-maintain JSON Schemas or rely only on golden payloads

Manual schemas can drift from Rust types; finite payloads alone cannot detect
every required/type/range change. Derived freshness, retained baselines, typed
reader tests, and deliberate structural mutations provide complementary checks.

## Consequences

The package is useful immediately through generation and credential-free tests.
The core production contract/adapter APIs, subjects, and existing fixture bytes
remain unchanged. Tooling gains a schema dependency graph, but no runtime service
or release image gains those packages. Generated artifacts support inspection
without executing Rust tooling; generation remains reproducible.

The conservative policy may reject changes that a more powerful compatibility
analysis could permit. That is intentional: review a new immutable identity or
an explicit policy extension instead of silently accepting an unfamiliar rule.
Payload validation cannot prove business compatibility or delivery recovery.

## Compatibility and migration

This introduces only new synthetic contracts. Existing messages, schemas,
persistence, error codes, and callers need no migration. Retain baseline schemas
and golden envelopes when adding future contracts. Never regenerate baselines
to make a failed compatibility check green. Rollback removes the conformance
package and its gate together without rewriting product state; no state exists
in this slice.

## Security and operations

Runtime schemas are generated from compiled-in types, not input URIs. The raw
envelope boundary remains 256 KiB. Direct JSON-value/Serde callers must bound
their own input; schema validation is not a replacement for parser input limits.
Validation errors and payload `Debug` expose only bounded categories, not raw
values, rejected URIs, or validator text. Telemetry, persistence, external
providers, and live trade execution are absent.

## Validation

The protected T2 `message-schema-contracts` target checks all current artifacts,
frozen baselines, golden envelope round trips, positive/negative payload cases,
old/new optional-field readers, and required/type/range/enum/unknown-keyword
mutations. It runs with serde_json/preserve_order as well as ordinary workspace
tests. Missing, renamed, duplicated, or extra protected tests fail discovery.
The same runner owns generation routing and default acceptance; no parallel CI
check list is added. Broker, transaction, process-restart, and exporter proofs
remain separate steps in the [M02 plan](../development/plans/m02-completion.md).
