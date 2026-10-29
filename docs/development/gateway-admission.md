# Durable local Gateway admission

## TL;DR

- Explicit `serve` mode accepts bounded POST requests for one trusted local operator.
- HTTP 202 follows a committed request/outbox transaction; it does not mean completion.
- Retry the same scope/request ID after lost responses or 503; changed intent is 409.
- Serving requires a separately prepared Gateway-owned PostgreSQL schema.

## Local operator profile

The Gateway's descriptor commands (`describe`, `self-check`, help and version)
continue to run without serving configuration. `edgeagent-gateway serve` requires:

| Variable | Meaning |
|---|---|
| `EDGEAGENT_GATEWAY_LISTEN` | Explicit loopback socket address, for example `127.0.0.1:8080` |
| `EDGEAGENT_POSTGRES_URL` | Loopback PostgreSQL URL without query/fragment overrides |
| `EDGEAGENT_GATEWAY_SCHEMA` | Prepared private schema, `gateway_` prefix, lowercase ASCII/digits/underscore, at most 63 bytes |
| `EDGEAGENT_GATEWAY_SCOPE` | Operator identity token, 1–64 ASCII letters/digits/period/underscore/hyphen |

All local processes are trusted as the configured operator. There is no public
login, proxy/forwarded identity or multi-user authorization. Do not expose or tunnel
the listener. Origin headers and nonlocal Host names are rejected; no CORS is
provided. A public profile requires separate identity/security review. SIGINT or
SIGTERM stops intake gracefully. No worker, provider call or relay is activated.

## POST contract

POST `/v1/research-requests` with `Content-Type: application/json` and the existing
[`ResearchRequest`](../../crates/contracts/src/research.rs) DTO (without a record
wrapper). JSON whitespace/key order may vary; unknown/duplicate fields fail.
The maximum body size is 131072 bytes. Scope must match the operator configuration.
Instrument IDs are sorted unique nonzero 32-character lowercase hexadecimal IDs;
implementation/configuration are nonzero 64-character lowercase hex digests.
The existing record contract validates cutoff, lookback and all remaining bounds.
The supplied cutoff is research decision time; command occurrence time is database
UTC admission time. No evidence availability or calculation is asserted by admission.

Success (including identical retry) is HTTP 202 with JSON
`{"request_id":"request-1","scope":"operator","status":"accepted"}`.
[Gateway queries](gateway-queries.md) optionally expose projector observations; admission
adds no Location link. Invalid input is 400,
foreign scope/browser Origin/nonlocal Host is 403, changed scoped ID content is
409, oversized body is 413, unsupported media type is 415, unavailable/corrupt
storage is 503. No raw error/payload is returned. Retry uncertain outcomes with the
same scoped ID/content; a timeout can occur after a successful database commit.

## Persistence and rollback

Apply [Gateway admission migration](../../services/gateway/migrations/0001_gateway_requests.sql)
and all [outbox migrations](../../crates/outbox-postgres/migrations/0001_message_outbox.sql)
in the same new Gateway-owned private schema. Startup checks table presence and
never migrates. Use a restricted runtime role: SELECT/INSERT on admission, required
outbox permissions, no DDL/TRUNCATE or admission update/delete/trigger-disable.
Do not point the service at Research's private completion tables.

Admission stores canonical request and exact command bytes. A scoped primary key
serializes concurrent retries; a separate READ COMMITTED read sees the winning
commit. Exact equality reuses the original command; changed content conflicts.
New admissions use the MarketData-owned fulfil-research-evidence definition, binding Gateway source, scoped subject/partition and
request digest. Fresh admission requires a fresh outbox row. Retained PR7 EVALUATE commands remain unchanged and retryable. Retained commands
must have intact routing/bytes; integrity failure refuses acknowledgement.
Outbox rows must remain for the full admission lifetime; independent pruning is
unsupported. Retention policy and relay are later slices. Rollback stops serving
and preserves admission/outbox rows for a compatible version.

The pool has eight connections and a five-second acquire timeout. Each transaction
uses five-second lock and fifteen-second statement timeouts; the HTTP admission
future has a twenty-second deadline. External calls stay outside the transaction.

## Verification

`cargo xtask verify --check gateway-http --python python3` executes credential-free
loopback HTTP/configuration tests. With an explicit loopback
`EDGEAGENT_POSTGRES_URL`, `cargo xtask verify --check gateway-admission --python python3`
executes four protected PostgreSQL proofs, including HTTP against real transactions.
They create/drop only unique owned schemas. The existing local-platform CI profile
includes them; default verification protects their exact discovery identities.

The canonical `sqlx-metadata` gate assembles Gateway migrations before preparing
workspace queries. It requires the pinned SQLx CLI and isolated Compose profile;
missing tools must be reported as not run. Live macro compilation against a
disposable database is supplementary evidence, not the canonical gate.
