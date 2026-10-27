# ADR-0072: Authenticate stream owner inputs and reserve namespaces

- Status: Accepted
- Date: 2026-10-27
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Install complete sealed admission, provider profile and derived identity claims independently.
- Restricted workers load actual protected bytes and compare their complete candidate.
- Reserve namespaces across accounts; erase private inputs only after confirmed writer quiescence.
- Keep namespace tombstones; add no runtime, companion publication or vendor activation.

## Context

PR20C1 proves deterministic interpretation, not independent approval. A worker can
construct a matching profile digest, and local profile uniqueness cannot prevent a
different profile assigning the same namespace to another logical fact scope. The
approved PR20C2 split separates authenticated inputs, companion persistence and runtime.

## Decision

PR20C2A introduces a bounded versioned neutral private input record. Preserve full
sealed session/authority/selection bytes, complete native profile bytes, explicit
exclusive expiry and complete namespace-to-scope claims. Derive IEX claims from the
actual profile and exact selection mappings in the HTTP adapter. Persistence depends
on the neutral application record, never provider-specific types. Matching bytes
authenticate owner installation, not the external truth of supplied reference catalogs.

The authenticated account/admission owner installs an immutable record. PostgreSQL
serializes cross-account namespace reservations and maintains a retained bijection:
one namespace per complete logical scope and one scope per namespace. Multiple
profiles may reference the same pair. Conflict, changed retry or failed validation
rolls back every claim and membership. No operation renews expiry.

Workers receive narrow EXECUTE-only loading. Early-bound SECURITY DEFINER functions
check actual LOGIN identity, owner-role membership (including NOINHERIT), write-capable
roles, current account/admission/retention denial, expiry and exact current namespace
membership. Rust decodes the actual stored record, compares full expected bytes and
rechecks current authority, cancellation and licensed lifetime using database time.
The returned data snapshot grants no reusable authorization; future runtime work
must revalidate controls before each operation and compose restore fences separately.

After independent storage-writer quiescence, the owner can erase private inputs and
unlink namespace membership. Retain shared scope bytes while another profile needs
them; erase unreferenced scope bytes and leave only opaque permanent namespace
tombstones. Never reopen those namespace IDs, even for the original scope. Once
licensed scope history is erased, this layer cannot reconstruct historical dedup;
any new acquisition/identity epoch requires a separately reviewed owner admission.

## Alternatives considered

- Checksum-only approval: a worker can reproduce it without independent installation.
- Worker-editable JSON configuration: merges approval and execution authority.
- Namespace uniqueness within each profile: misses cross-profile/account collisions.
- Reuse namespaces after erasing scope history: can alias previously published facts.
- Store provider DTOs in PostgreSQL adapters: reverses dependency direction.
- Compose the entire runtime now: mixes authentication, publication and supervision proofs.

## Consequences

Private owner records duplicate complete bounded values deliberately. Control-plane
installation serializes on the namespace table OID and holds no external I/O. Input
retirement releases no connection generation, account slot, usage counter or evidence
object. Tombstones grow with the number of allocated namespaces, not incoming ticks.
Owner-approved catalog provenance remains a trusted configuration boundary.

## Compatibility and migration

Apply acquisition migration `0010_stream_owner_inputs.sql` after `0009_stream_recovery.sql`.
It adds private tables/functions without changing existing public messages or artifact
formats. Install inputs before a future worker attempts this path. Existing segment
APIs do not automatically enforce this new contract; PR20C2C must compose it.
Rollback consumers first; preserve private tables/tombstones rather than deleting
identity history. PR20C2B must atomically bind native companion provenance to canonical
publication before PR20C2C composes the offline runtime and readiness proofs.

## Security and operations

The service-private schema and its owner LOGIN are trusted administration boundaries.
Workers must not inherit owner credentials, schema CREATE or control table DML;
grant only `edgeagent_load_stream_inputs(TEXT)` for this path. Never log full input
or scope bytes. Explicit expiry is bounded by the capture window and current rights
deletion/canonical retention (at most 24 hours). Revocation denies new loads immediately
after the denying transaction commits; it cannot recall an already returned snapshot.
Retirement requires the owner's existing writers-quiescent assertion, not process
death, elapsed timeout or cancellation. No provider connection or credential is used.

## Validation

Pure proofs cover bounded canonical decoding, exact bytes, profile pinning, duplicate
claims, cancellation, exclusive expiry and redaction. Provider proofs derive claims
from complete IEX profiles. Isolated PostgreSQL proofs cover restricted LOGINs,
temporary shadow relations, NOINHERIT escalation, concurrent cross-account collisions,
changed-retry rollback, shared ownership, private erasure and non-reusable tombstones.
Protected proof inventories and SQLx migration assembly include this slice. Local
workspace compilation and strict Clippy run before delivery; CI must run the heavy
proofs and regenerate/check SQLx metadata against the actual migrated schema.
See [stream owner inputs](../development/stream-owner-inputs.md).
