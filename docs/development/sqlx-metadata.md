# SQLx offline query metadata

Inbox adapter SQL and outbox enqueue SQL use `sqlx::query!` or
`sqlx::query_file!`. The workspace commits their `.sqlx/` metadata, and
`.cargo/config.toml` defaults to `SQLX_OFFLINE=true`. Ordinary `cargo check`,
`cargo build`, Clippy, and tests therefore need neither PostgreSQL nor a
`.env` file. The database-backed CI gate regenerates the metadata and rejects
any diff; runtime PostgreSQL conformance tests remain a separate behavioral
check.

Metadata must be generated against a fresh, isolated PostgreSQL database with
**both** adapters' checked-in tables in the default `public` schema. The two
migration directories both start at version 1, so SQLx CLI's shared migration
ledger can track only one of them in a single schema. Run the outbox migrations
through SQLx CLI and apply the inbox SQL files directly with `psql`, in lexical
order. This is a metadata-only schema assembly, not the production migration
procedure. Service composition roots remain responsible for deployment
migrations and schema ownership.

On the local Compose platform, run these commands from the workspace root in
a POSIX shell. Use a fresh local database; existing or drifted tables may
produce misleading metadata. The example credentials are local-only.

```sh
make local-up
cargo install sqlx-cli --version '=0.9.0' --no-default-features --features postgres,rustls
export DATABASE_URL='postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent'
export SQLX_OFFLINE=false
cargo sqlx migrate run --source crates/outbox-postgres/migrations --no-dotenv
for migration in crates/inbox-postgres/migrations/*.sql; do
  docker compose --env-file deploy/local/.env.example -f deploy/local/compose.yaml exec -T postgres \
    psql -X -v ON_ERROR_STOP=1 -U edgeagent -d edgeagent < "$migration"
done
cargo sqlx prepare --workspace --no-dotenv -- --locked --all-targets
git status --short -- .sqlx
```

Review and commit every added, changed, or removed `.sqlx/query-*.json` with
the query or migration change. Never use production for metadata generation or
commit a real database URL. CI repeats the same schema assembly and runs
`cargo sqlx prepare` with SQLx CLI `0.9.0`, then fails on any `.sqlx/` diff.
