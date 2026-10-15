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

All production inbox and outbox SQL uses checked SQLx query macros. Outbox
enqueue and replay identity lookups use `sqlx::query!`; lease transitions and
the audited replay CTE use readable capability-owned `query_file!` /
`query_file_as!` statements. All eight outbox statements participate in the
same metadata workflow. The workspace commits their `.sqlx/` metadata, and
`.cargo/config.toml` defaults to `SQLX_OFFLINE=true`. Ordinary `cargo check`,
`cargo build`, Clippy, and tests therefore need neither PostgreSQL nor a
`.env` file. The database-backed CI gate regenerates the metadata and rejects
any diff; runtime PostgreSQL conformance tests remain a separate behavioral
check.

The outbox's capability-local `clippy.toml` rejects runtime query constructors
and unchecked macros in production. Isolated PostgreSQL conformance fixtures
have explicit test-only exemptions for schema administration and fault
injection. `cargo xtask verify --check sqlx-query-policy` runs real offline
Clippy fixtures: a checked macro must compile, while prohibited APIs must fail
with the expected lint, including renamed imports. This T2 check runs on both
CI platforms; it does not connect to PostgreSQL or replace the T3 metadata
freshness and runtime conformance proofs.

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

The action probes `cargo sqlx --no-dotenv --version`, which reports
`sqlx-cli-sqlx 0.9.0`. The standalone `sqlx --version` reports
`sqlx-cli 0.9.0`; these are different command banners for the same pinned
package. The probe requires a successful exit and the exact Cargo-plugin
banner, ignores ambient `.env` files, and rejects other versions. Failure
diagnostics distinguish an unsuccessful probe from an unexpected banner
without printing raw command output.

## Metadata changes and evidence boundaries

Review and commit every added, changed, or removed `.sqlx/query-*.json` with
the query or migration change. Never use production for metadata generation or
commit a real database URL. The command intentionally fails after generation
when metadata changes: review and commit the changes, then rerun it. CI invokes
the same canonical action and checks for an exactly empty
`git status --porcelain -- .sqlx` result. Runtime transaction, quarantine,
replay, and fencing guarantees remain separate local-platform conformance
checks.
