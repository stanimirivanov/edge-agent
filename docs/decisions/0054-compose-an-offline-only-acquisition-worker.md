# ADR-0054: Compose an offline-only acquisition worker

- Status: Accepted
- Date: 2026-10-24
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Compose existing acquisition ports for one pinned invented daily profile. Do not
activate vendors. PR11D/PR11E remain vendor prerequisites, not offline runtime gates.

## Context

Source replay, selection, budgets, publication and retention exist independently.
An executable zero-quota boundary is useful before vendor decisions. The maintainer
approved offline-only composition, not live qualification.

## Decision

MarketData wires PostgreSQL admission/receipt/inbox/outbox, retained filesystem
storage, NATS delivery and the existing relay. Private admission-v2 pins configuration;
legacy v1 remains readable. Changed compiled recipes fail closed. Public contracts
and inward application/domain dependencies remain unchanged.

Require finite explicit settings and loopback infrastructure. A local profile lock
excludes competing owners through shutdown. Initialize only a fresh root-derived
schema. The CLI is trusted local operator authority, not public authentication.
Recover before source work; ACK only confirmed commit or durable denial. Quarantine
poison before broker termination. Never infer rollback from lost confirmation.

Bound maintenance and require explicit writer quiescence after restart. Never clear
unknown account calls by timeout; this worker has no vendor or account-unblocking
path. Minimal lifecycle is a prerequisite from M10, which retains cloud/HA/restore
and authenticated live operational qualification.

## Alternatives considered

- Wait for both vendors: blocks zero-quota runtime unnecessarily.
- Generic provider registry now: risks unreviewed activation and authority expansion.
- Another bus/workflow engine: duplicates established event-spine guarantees.
- Timeout-based cleanup: races delayed writes and violates the retention fence.

## Consequences

One invented local profile is executable; live fidelity/distributed fencing are not
qualified. Uncommitted preparation may repeat and create inventoried orphans. Raw
credential/error sources stay out of logs. See the
[operator guide](../development/offline-worker.md) for explicit limits.

## Compatibility and migration

No wire changes or new production query metadata. Existing migrations initialize an
isolated namespace. Admission-v2 needs the new reader: stop intake before rollback,
retain a compatible binary for outstanding jobs and never reseal stored admissions.
Vendor activation still needs PR11D/PR11E, reviewed authority/fencing and PR15.

## Security and operations

The invented local profile is not a licensed-data entitlement. Reject unknown
providers before dependencies; require loopback infrastructure and explicit limits.
Report bounded policy categories/SQLSTATE, never source-chain messages or secrets.
Stop readiness on dependency failure. Retain uncertain objects/account blocks until
separate owner confirmation; local locks cannot fence multi-host or remote writers.

## Validation

Portable proofs protect mode rejection, pinned replay/cancellation, private v1/v2
compatibility and non-inferred cleanup authority. Owned process tests cover receipt
recovery, relay and revocation/deletion. PR14 owns injected death, broker restart
and acknowledgement loss; no such qualification is implied here.
