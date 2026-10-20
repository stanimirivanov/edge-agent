//! Isolated, owned database and real process fixtures. Financial data is synthetic.
use edgeagent_contracts::{MessageEnvelope, simulation::OrderAdmission};
use edgeagent_event_spine_conformance::accounts::Scenario;
use edgeagent_inbox_postgres::PostgresInbox;
use edgeagent_outbox_postgres::PostgresOutbox;
use sqlx::{Connection, Executor, PgConnection, Row, postgres::PgConnectOptions};
use std::{
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};
pub type Error = Box<dyn std::error::Error + Send + Sync>;
static NEXT: AtomicU64 = AtomicU64::new(1);
pub struct Database {
    connection: PgConnection,
    schema: String,
    role: String,
    url: String,
    case: String,
    next: u64,
}
pub struct Worker {
    pub application: String,
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}
impl Database {
    pub async fn new(case: &str) -> Result<Self, Error> {
        let url = std::env::var("EDGEAGENT_POSTGRES_URL")?;
        if url != "postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent" {
            return Err(
                "account proof requires the documented disposable local PostgreSQL fixture".into(),
            );
        }
        let schema = format!(
            "p08_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let role = format!("{schema}_worker");
        let options: PgConnectOptions = url.parse()?;
        let mut connection = PgConnection::connect_with(&options).await?;
        ddl(&mut connection, format!("CREATE SCHEMA {schema}")).await?;
        ddl(&mut connection, format!("SET search_path TO {schema}")).await?;
        let mut database = Self {
            connection,
            schema,
            role,
            url,
            case: case.into(),
            next: 0,
        };
        let setup = database.initialize().await;
        if let Err(error) = setup {
            let _ = database.cleanup().await;
            return Err(error);
        }
        Ok(database)
    }
    async fn initialize(&mut self) -> Result<(), Error> {
        for migration in PostgresOutbox::MIGRATIONS
            .iter()
            .chain(PostgresInbox::MIGRATIONS.iter())
        {
            sqlx::raw_sql(*migration)
                .execute(&mut self.connection)
                .await?;
        }
        sqlx::raw_sql(edgeagent_simulation_postgres::MIGRATION_SQL)
            .execute(&mut self.connection)
            .await?;
        let mut initializer = self.command(0, "initialize", 1, "initialize", false)?;
        initializer.success().await?;
        // Runtime cannot create schema, provision grants, edit quotes or mint cash.
        ddl(
            &mut self.connection,
            format!(
                "CREATE ROLE {} LOGIN PASSWORD 'p08-public-fixture-only'",
                self.role
            ),
        )
        .await?;
        ddl(
            &mut self.connection,
            format!("GRANT USAGE ON SCHEMA {} TO {}", self.schema, self.role),
        )
        .await?;
        ddl(
            &mut self.connection,
            format!(
                "GRANT SELECT ON ALL TABLES IN SCHEMA {} TO {}",
                self.schema, self.role
            ),
        )
        .await?;
        // Admission is append-only. Relay/replay and provisioning capabilities
        // are not granted to this worker.
        ddl(
            &mut self.connection,
            format!(
                "GRANT INSERT ON edgeagent_message_inbox, edgeagent_message_outbox, simulation_order, simulation_reservation TO {}",
                self.role
            ),
        )
        .await?;
        ddl(&mut self.connection,format!("GRANT UPDATE(revision,input_revision,risk_checkpoint,run_checkpoint) ON simulation_account TO {}",self.role)).await?;
        Ok(())
    }
    fn command(
        &mut self,
        index: usize,
        mode: &str,
        principal: u128,
        id: &str,
        runtime: bool,
    ) -> Result<Worker, Error> {
        self.next += 1;
        let application = format!("{}_{}", self.schema, self.next);
        let mut command = Command::new(env!("CARGO_BIN_EXE_account-worker"));
        command
            .args([&self.case, &index.to_string(), mode, &principal.to_string()])
            .env("EDGEAGENT_POSTGRES_URL", &self.url)
            .env("P08_SCHEMA", &self.schema)
            .env("P08_APPLICATION", &application)
            .env("P08_MESSAGE_ID", id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if runtime {
            command.env("P08_ROLE", &self.role);
        }
        let mut child = command.spawn()?;
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().ok_or("worker stdout unavailable")?);
        Ok(Worker {
            application,
            child,
            input,
            output,
        })
    }
    pub fn spawn(
        &mut self,
        index: usize,
        mode: &str,
        principal: u128,
        id: &str,
    ) -> Result<Worker, Error> {
        self.command(index, mode, principal, id, true)
    }
    pub fn spawn_conflict(&mut self, index: usize) -> Result<Worker, Error> {
        // This setting belongs to this child only; no process-global env mutation.
        self.next += 1;
        let application = format!("{}_{}", self.schema, self.next);
        let mut child = Command::new(env!("CARGO_BIN_EXE_account-worker"))
            .args([&self.case, &index.to_string(), "apply", "1"])
            .env("EDGEAGENT_POSTGRES_URL", &self.url)
            .env("P08_SCHEMA", &self.schema)
            .env("P08_ROLE", &self.role)
            .env("P08_APPLICATION", &application)
            .env("P08_MESSAGE_ID", "conflicting-receipt")
            .env("P08_CONFLICT", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().ok_or("worker stdout unavailable")?);
        Ok(Worker {
            application,
            child,
            input,
            output,
        })
    }
    pub async fn wait_for_root_lock(&mut self, application: &str) -> Result<(), Error> {
        tokio::time::timeout(Duration::from_secs(10),async {
            loop {
                let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock')").bind(application).fetch_one(&mut self.connection).await?;
                if blocked {return Ok::<_,Error>(());}
                tokio::task::yield_now().await;
            }
        }).await??;
        Ok(())
    }
    pub async fn assert_counts(
        &mut self,
        orders: i64,
        reservations: i64,
        outbox: i64,
        inbox: i64,
    ) -> Result<(), Error> {
        let actual=sqlx::query("SELECT (SELECT COUNT(*) FROM simulation_order) AS orders,(SELECT COUNT(*) FROM simulation_reservation) AS reservations,(SELECT COUNT(*) FROM edgeagent_message_outbox) AS outbox,(SELECT COUNT(*) FROM edgeagent_message_inbox) AS inbox").fetch_one(&mut self.connection).await?;
        assert_eq!(
            (
                actual.try_get::<i64, _>("orders")?,
                actual.try_get::<i64, _>("reservations")?,
                actual.try_get::<i64, _>("outbox")?,
                actual.try_get::<i64, _>("inbox")?
            ),
            (orders, reservations, outbox, inbox)
        );
        Ok(())
    }
    pub async fn assert_reserved(&mut self, cash: i64, quantity: i64) -> Result<(), Error> {
        let row=sqlx::query("SELECT COALESCE(SUM(cash_cents),0)::BIGINT AS cash,COALESCE(SUM(quantity),0)::BIGINT AS quantity FROM simulation_reservation").fetch_one(&mut self.connection).await?;
        assert_eq!(
            (
                row.try_get::<i64, _>("cash")?,
                row.try_get::<i64, _>("quantity")?
            ),
            (cash, quantity)
        );
        Ok(())
    }
    pub async fn outcome_bytes(&mut self, index: usize) -> Result<Vec<u8>, Error> {
        let scenario = Scenario::new(&self.case)?;
        let request = scenario.request(index)?;
        Ok(
            sqlx::query_scalar("SELECT outcome FROM simulation_order WHERE order_id=$1")
                .bind(request.identity().1)
                .fetch_one(&mut self.connection)
                .await?,
        )
    }
    pub async fn outcome(&mut self, index: usize) -> Result<OrderAdmission, Error> {
        Ok(MessageEnvelope::from_json(&self.outcome_bytes(index).await?)?.payload()?)
    }
    pub async fn checkpoints(&mut self) -> Result<(i64, i64, Vec<u8>, Vec<u8>), Error> {
        Ok(sqlx::query_as(
            "SELECT revision,input_revision,risk_checkpoint,run_checkpoint FROM simulation_account",
        )
        .fetch_one(&mut self.connection)
        .await?)
    }
    pub async fn refresh_valuations(&mut self) -> Result<(), Error> {
        let mut transaction = self.connection.begin().await?;
        sqlx::query("SELECT account FROM simulation_account FOR UPDATE")
            .fetch_one(&mut *transaction)
            .await?;
        sqlx::query("UPDATE simulation_valuation SET valued_seconds=12")
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }
    pub async fn tamper_quote(&mut self) -> Result<(), Error> {
        let mut transaction = self.connection.begin().await?;
        sqlx::query("SELECT account FROM simulation_account FOR UPDATE")
            .fetch_one(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE simulation_quote SET canonical=set_byte(canonical,0,0) WHERE input_revision=1",
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
    async fn cleanup(&mut self) -> Result<(), Error> {
        self.connection.execute("SET search_path TO public").await?;
        ddl(
            &mut self.connection,
            format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema),
        )
        .await?;
        ddl(
            &mut self.connection,
            format!("DROP ROLE IF EXISTS {}", self.role),
        )
        .await?;
        Ok(())
    }
    pub async fn finish(
        mut self,
        result: std::thread::Result<Result<(), Error>>,
    ) -> Result<(), Error> {
        let cleanup = self.cleanup().await;
        match result {
            Ok(result) => result.and(cleanup),
            Err(panic) => {
                cleanup?;
                std::panic::resume_unwind(panic)
            }
        }
    }
}
async fn ddl(connection: &mut PgConnection, sql: String) -> Result<(), Error> {
    // Only numeric PID/counter-owned identifiers enter this fixture-only DDL.
    // No user strings, evidence or financial values enter SQL identifiers.
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(connection)
        .await?;
    Ok(())
}
impl Worker {
    pub async fn stage(&mut self, expected: &str) -> Result<(), Error> {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(15), self.output.read_line(&mut line)).await??;
        if line.trim() != expected {
            return Err("worker did not reach requested barrier".into());
        }
        Ok(())
    }
    pub async fn release(&mut self) -> Result<(), Error> {
        let input = self.input.as_mut().ok_or("worker stdin unavailable")?;
        input.write_all(b"x").await?;
        input.flush().await?;
        Ok(())
    }
    pub async fn success(&mut self) -> Result<(), Error> {
        let status = tokio::time::timeout(Duration::from_secs(30), self.child.wait()).await??;
        if !status.success() {
            return Err("account worker unexpectedly failed".into());
        }
        Ok(())
    }
    pub async fn failure(&mut self, code: &str) -> Result<(), Error> {
        self.stage(&format!("REJECT {code}")).await?;
        let status = tokio::time::timeout(Duration::from_secs(30), self.child.wait()).await??;
        if status.success() {
            return Err("account worker unexpectedly succeeded".into());
        }
        Ok(())
    }
    pub async fn kill(&mut self) -> Result<(), Error> {
        self.child.kill().await?;
        self.child.wait().await?;
        Ok(())
    }
}
