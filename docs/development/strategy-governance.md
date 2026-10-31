# Local strategy approval authority

## TL;DR

- M05-PR3 installs exact M04 profiles, then appends approval and terminal retirement.
- Every fresh publication now requires complete validated execution and exact durable approval.
- Governance is local schema-owner authority, not multi-user production authorization.

## Identity and authority

The maintainer-approved compatibility choice binds the existing scope, implementation
and configuration digests, lookback, maximum evidence age, TTL, formula and renderer.
[Profile](../../crates/contracts/src/strategy_governance.rs) bytes are canonical JSON
with final LF and no unknown/duplicate fields. Profile v1 is the explicitly named
legacy schema table contract; it is not the full [PR2 bundle](strategy-policy.md).
Only the currently compiled momentum/renderer baseline is supported. Zero revision
sentinels, unknown formulas, invalid bounds and alias names reject. Definition rows
are immutable, including when another definition uses the same key.

Administration requires the authenticated PostgreSQL `session_user` to own the
current private schema. The service records that actor and transaction time; it
accepts no actor supplied by a request, environment label or model. Shared owner
credentials confer shared local authority: this is not separation of duties or
production multi-user approval. Use separately controlled operator credentials and
read/write grants for consumers before any wider deployment. A schema owner can
administer its database, including disabling triggers; owner compromise is outside
this local authority boundary.

## Installation and execution

Apply inbox/outbox, completion and lifecycle migrations followed by
[`0003_strategy_governance.sql`](../../crates/research-postgres/migrations/0003_strategy_governance.sql)
in the same prepared Research schema. Migration never automatically approves anything.
With the existing loopback `EDGEAGENT_POSTGRES_URL`, `EDGEAGENT_RESEARCH_SCHEMA` and
`EDGEAGENT_RESEARCH_SCOPE`, pass the reviewed exact canonical profile bytes on stdin
to these Research binary commands:

- `offline-governance-install`: append revision 1, installed but not approved.
- `offline-governance-approve`: append revision 2 for the installed baseline.
- `offline-governance-retire`: append terminal revision 3; reapproval is forbidden.

Repeating the latest identical action with the same authenticated actor is an exact
retry. Skipped transitions, stale actions and changed definitions reject atomically.
No general promotion of another strategy is available; PR16 adds validation evidence.

`offline-once` and `offline-governed-once` accept the existing evidence delivery on stdin.
Fresh execution requires the complete canonical file selected by
`EDGEAGENT_RESEARCH_EXECUTION_CONFIGURATION`, matching existing owner settings.
Lookback comes from that file; the old lookback environment setting no longer selects it.
The worker requires approval before owner reads and holds shared scope-advisory/profile
locks through completion; administration acquires the corresponding exclusive locks. If retirement commits first, new completion rejects;
if completion commits first, retirement stops later completions without rewriting it.
No external owner call runs inside a transaction. Changed/experimental/unknown/retired
profiles fail closed without a new completion, inbox effect or publication intent.

Receipt-first recovery returns original committed bytes even after retirement and
without evidence reads. That is receipt recovery, not permission for new computation.
Historical read-only replay retains its existing behavior. Profile approval is not
entitlement, strategy performance validation, or an account-risk override.

## Rollout and rollback

[Validated publication](validated-publication.md) now requires a complete canonical
execution configuration and approval of its new digest for every fresh completion.
Legacy modes no longer bypass approval/validation. The profile format and explicit
administration remain unchanged; historical receipt recovery does not transfer
approval to a new configuration. Apply migration 0004 and provision the companion
route/reader upgrade before new publication.

Retain definitions and retirement history on rollback. Reverting to a legacy binary
removes enforcement, so stop governed ingestion before rollback; never delete retirement
rows to restore availability. No artifact or historical receipt changes.

## Verification

Run `cargo xtask verify --check research-persistence --python python3` and
`cargo xtask verify --check research-worker-postgres --python python3` with an isolated
loopback PostgreSQL URL. Tests cover unknown/experimental profiles, exact retries,
conflicts, transitions, terminal retirement, restart recovery, ownership refusal,
immutable history and retirement fencing. Run the SQLx metadata gate where its pinned
CLI and Docker profile are available, followed by the full default verification.
