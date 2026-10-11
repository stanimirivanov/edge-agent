# SQLx offline query metadata

## TL;DR

- Checked-in `.sqlx/` metadata keeps normal builds credential-free and offline.
- `cargo xtask verify sqlx-metadata` regenerates and checks metadata against only
  the explicitly authorized isolated local Compose PostgreSQL profile.
- The canonical action checks SQLx CLI `0.9.0`, assembles both adapters' schemas,
  and fails if generated `.sqlx/` metadata differs from the working tree.
- This proves query/schema freshness, not runtime transaction behavior or a
  production migration procedure.

## Offline build contract

Inbox adapter SQL and outbox enqueue SQL use `sqlx::query!` or
`sqlx::query_file!`. The workspace commits their `.sqlx/` metadata, and
`.cargo/config.toml` defaults to `SQLX_OFFLINE=true`. Ordinary `cargo check`,
`cargo build`, Clippy, and tests therefore need neither PostgreSQL nor a
`.env` file. The database-backed CI gate regenerates the metadata and rejects
any diff; runtime PostgreSQL conformance tests remain a separate behavioral
check.

## Schema assembly and isolation

Metadata must be generated against a fresh, isolated PostgreSQL database with
**both** adapters' checked-in tables in the default `public` schema. The two
migration directories both start at version 1, so SQLx CLI's shared migration
ledger can track only one of them in a single schema. Run the outbox migrations
through SQLx CLI and apply the inbox SQL files directly with `psql`, in lexical
order. This is a metadata-only schema assembly, not the production migration
procedure. Service composition roots remain responsible for deployment
migrations and schema ownership.

The action refuses any `DATABASE_URL` other than the exact checked-in local
Compose URI. It also requires `SQLX_OFFLINE=false` and explicit
`EDGEAGENT_SQLX_METADATA_LOCAL=1` authorization before changing schema or
generating metadata. These checks do not prove the database is fresh: use a
fresh local profile without retained or drifted tables. Never target a shared
or production database.

## Canonical verification command

On the local Compose platform, run these commands from the workspace root in a
POSIX shell. The example credentials are public local-only fixtures.

```sh
make local-up
cargo install sqlx-cli --version '=0.9.0' --no-default-features --features postgres,rustls
export DATABASE_URL='postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent'
export SQLX_OFFLINE=false
export EDGEAGENT_SQLX_METADATA_LOCAL=1
cargo xtask verify sqlx-metadata
```

PowerShell users set the same three environment values with `$env:NAME='value'`
and run the same canonical Cargo command. The action launches fixed argument
vectors and pipes migration bytes directly into Compose `psql`; no POSIX shell
or manually maintained migration loop is required.

## Metadata changes and evidence boundaries

Review and commit every added, changed, or removed `.sqlx/query-*.json` with
the query or migration change. Never use production for metadata generation or
commit a real database URL. The command intentionally fails after generation
when metadata changes: review and commit the changes, then rerun it. CI invokes
the same canonical action and checks for an exactly empty
`git status --porcelain -- .sqlx` result. Runtime transaction, quarantine,
replay, and fencing guarantees remain separate local-platform conformance
checks.
