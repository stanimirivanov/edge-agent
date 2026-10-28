# Authenticated stream owner inputs

## TL;DR

- PR20C2A installs complete private input bytes independently of the worker.
- Derive namespace claims with `IexCanonicalProfile::owner_inputs`, not a parallel map.
- Recheck current controls whenever loading; a returned snapshot is not a capability.
- This is offline infrastructure, not streaming runtime, publication or readiness.

## Install and load

An independent owner first configures the account and admits the sealed streaming
session using existing owner methods. Supply actual complete authority catalogs and
selection witnesses, the complete `IexCanonicalProfile`, and an explicit exclusive
expiry. `owner_inputs(session, selection, expires)` derives all logical namespace
claims from that profile and requires exact selected mapping coverage. The neutral
`StreamOwnerInputs` retains complete session/profile bytes and claims, not just hashes.
Current catalog provenance is trusted owner configuration, not self-authenticating data.

Use `PostgresStreamingLedger::install_stream_inputs` under the same authenticated
account/admission owner LOGIN. Installation atomically reserves namespaces and links
the immutable record. Identical retries succeed without extending expiry; any changed
record or contradictory claim fails and rolls back every new reservation. All accounts
in this service-private schema share namespace ownership. Namespace IDs are nonzero
u64 values encoded as fixed lowercase hex, including values beyond signed bigint.

Each IEX logical scope includes source ID, instrument ID, session date and trade or
minute-bar kind; trade scopes also bind exchange/tape. Profile/reference revisions
cannot generate a different identity for the same retained scope. Both directions
are unique while retained: a namespace cannot change scope, and a scope cannot move
to another namespace. Several profiles may share exactly the same pair.

A restricted worker re-derives its candidate from the typed complete profile and calls
`load_stream_inputs(session, expected, catalogs, cancel)`. The database returns the
actual independently installed private bytes only after checking current controls and
role independence. Rust decodes them, compares every byte and rechecks current
authority/selection, cancellation, expiry and retention. Reject missing, retired,
changed, revoked or expired inputs. Do not turn this DTO into an activation flag.
PR20C2C must compose fresh checks with restore barriers before externally visible work.

## Migration and roles

Apply `crates/market-data-postgres/migrations/0010_stream_owner_inputs.sql` after 0009.
New tables are `edgeagent_stream_owner_inputs`, `edgeagent_stream_namespaces` and
`edgeagent_stream_namespace_members`; none grants PUBLIC access. Keep all three under
the independent schema owner. Add only this grant to the existing restricted worker:

```sql
GRANT EXECUTE ON FUNCTION edgeagent_load_stream_inputs(TEXT) TO stream_worker;
```

Do not grant workers `edgeagent_stream_owner_inputs_control`, table/column DML,
schema CREATE or membership in owner/write-capable roles, even with NOINHERIT.
Early-bound function bodies and a pinned pg_catalog search path resist temporary
relation shadowing. Existing account/admission/retention worker guards still apply.
The load check scans role membership, including indirect write/column privileges.

Transactions lock account, admission, session and retention in existing order.
Installation/retirement then takes a transaction advisory lock keyed by the namespace
table OID, serializing control-plane claims across accounts. No external I/O occurs
under it. SQL constraints abort the entire call for invalid/changed installs, including
direct SQL/autocommit attempts. Do not add implicit retries or switch lock order.

## Expiry and private retirement

Expiry must fit the original capture window and current rights deletion/canonical
retention limits, capped at 24 hours. Nothing renews it. Denial/expiry blocks loading;
an already returned snapshot cannot be recalled and must never substitute for fresh
authorization at a later effect boundary.

After separately confirming storage writers quiescent with
`confirm_stream_writers_quiescent`, the owner calls `retire_stream_inputs`. This erases
the full private record and unlinks its memberships. Other profiles retain shared
scope bytes until their own retirement. Unreferenced scope bytes are erased and
opaque namespace tombstones remain permanently closed. No provider connection,
unresolved generation, quota counter or immutable object is released by this method.

After licensed scope history is erased, old scope dedup cannot be reconstructed.
Never reuse its namespace. A new acquisition/identity epoch needs separate review
and admission; this API does not silently create one. Keep tombstones on rollback.

## Verification and follow-up

`cargo xtask verify --check streaming-segment-contracts` covers neutral canonical
records; `cargo xtask verify --check iex-canonical-acceptance` covers derived claims.
CI runs `cargo xtask verify --check streaming-publication-postgres` for isolated role,
concurrency and retirement proofs, and `cargo xtask verify --check sqlx-metadata` for
actual schema/cache freshness. The new checked-query cache descriptors are supplied
for offline compilation; actual migrated-schema validation is required in CI.
Default acceptance remains `cargo xtask verify`.

PR20C2B must persist a versioned native companion atomically with canonical publication
under shared retention. PR20C2C then composes bounded offline workers, restore/recovery,
publication and domain readiness. No vendor activation or live qualification ships here.
See [ADR-0072](../decisions/0072-authenticate-stream-owner-inputs-and-reserve-namespaces.md).
