use std::{
    error::Error,
    future::Future,
    panic::AssertUnwindSafe,
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use edgeagent_inbox_postgres::PostgresInbox;
use edgeagent_outbox_postgres::PostgresOutbox;
use futures_util::FutureExt;
use sqlx::{ConnectOptions, Connection, PgConnection, postgres::PgConnectOptions};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

pub type TestError = Box<dyn Error + Send + Sync>;
pub type TestResult = Result<(), TestError>;
static NEXT: AtomicU64 = AtomicU64::new(1);
const SCHEMA: &str = include_str!("../../fixtures/process-schema.sql");
const DEADLINE: Duration = Duration::from_secs(10);

pub struct Database {
    pub admin: PgConnection,
    options: PgConnectOptions,
    pub producer: String,
    pub consumer: String,
    roles: Vec<String>,
    schemas: Vec<String>,
}

impl Database {
    async fn new() -> Result<Self, TestError> {
        let options: PgConnectOptions = std::env::var("EDGEAGENT_POSTGRES_URL")?.parse()?;
        let admin = tokio::time::timeout(DEADLINE, PgConnection::connect_with(&options)).await??;
        let id = format!(
            "m02_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        Ok(Self {
            admin,
            options,
            producer: format!("{id}_p"),
            consumer: format!("{id}_c"),
            roles: vec![],
            schemas: vec![],
        })
    }

    async fn prepare(&mut self) -> TestResult {
        for schema in [self.producer.clone(), self.consumer.clone()] {
            // Names originate only from PID/counter and fixed ASCII suffixes.
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
                .execute(&mut self.admin)
                .await?;
            self.schemas.push(schema.clone());
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SET search_path TO {schema}")))
                .execute(&mut self.admin)
                .await?;
            for migration in PostgresOutbox::MIGRATIONS {
                sqlx::raw_sql(migration).execute(&mut self.admin).await?;
            }
            if schema == self.consumer {
                for migration in PostgresInbox::MIGRATIONS {
                    sqlx::raw_sql(migration).execute(&mut self.admin).await?;
                }
            }
            // Only create the owning service's table, not a shared writable model.
            let (acceptance, probe) = SCHEMA
                .split_once("CREATE TABLE IF NOT EXISTS conformance_probe")
                .ok_or("fixture schema boundary absent")?;
            if schema == self.producer {
                sqlx::raw_sql(acceptance).execute(&mut self.admin).await?;
            } else {
                sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                    "CREATE TABLE IF NOT EXISTS conformance_probe{probe}"
                )))
                .execute(&mut self.admin)
                .await?;
            }
            for relay in [false, true] {
                let role = if relay {
                    format!("{schema}_r")
                } else {
                    schema.clone()
                };
                sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                    "CREATE ROLE {role} LOGIN PASSWORD 'fixture-only'"
                )))
                .execute(&mut self.admin)
                .await?;
                self.roles.push(role.clone());
                let grants = if relay {
                    "GRANT SELECT, UPDATE ON edgeagent_message_outbox TO"
                } else {
                    "GRANT SELECT, INSERT ON edgeagent_message_outbox TO"
                };
                sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                    "GRANT USAGE ON SCHEMA {schema} TO {role}; {grants} {role}"
                )))
                .execute(&mut self.admin)
                .await?;
                if !relay {
                    let tables = if schema == self.producer {
                        "conformance_acceptance"
                    } else {
                        "conformance_probe, edgeagent_message_inbox, edgeagent_message_quarantine"
                    };
                    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                        "GRANT SELECT, INSERT ON {tables} TO {role}"
                    )))
                    .execute(&mut self.admin)
                    .await?;
                    if schema == self.consumer {
                        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                            "GRANT UPDATE ON edgeagent_message_quarantine TO {role}"
                        )))
                        .execute(&mut self.admin)
                        .await?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn runtime_options(&self, consumer: bool, relay: bool) -> PgConnectOptions {
        let schema = if consumer {
            &self.consumer
        } else {
            &self.producer
        };
        let role = if relay {
            format!("{schema}_r")
        } else {
            schema.clone()
        };
        self.options
            .clone()
            .username(&role)
            .password("fixture-only")
    }

    pub async fn count(&mut self, consumer: bool, table: &str) -> Result<i64, TestError> {
        // Callers supply only literal fixture table names, never transport data.
        let schema = if consumer {
            &self.consumer
        } else {
            &self.producer
        };
        Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {schema}.{table}"
        )))
        .fetch_one(&mut self.admin)
        .await?)
    }

    pub async fn worker(
        &self,
        role: &str,
        consumer: bool,
        url: &str,
        commit: bool,
    ) -> Result<String, TestError> {
        let schema = if consumer {
            &self.consumer
        } else {
            &self.producer
        };
        let component = if consumer { "research" } else { "gateway" };
        let database_url = self
            .runtime_options(consumer, role == "relay")
            .to_url_lossy()
            .to_string();
        let mut command = Command::new(env!("CARGO_BIN_EXE_conformance-worker"));
        // Do not forward administrator URLs, provider keys or unrelated ambient
        // credentials. Windows still needs its OS directory for native services.
        command.env_clear();
        if let Some(directory) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", directory);
        }
        let mut child = command
            .arg(role)
            .env("EDGEAGENT_POSTGRES_URL", database_url)
            .env("EDGEAGENT_PROOF_SCHEMA", schema)
            .env("EDGEAGENT_PROOF_NAMESPACE", "m02-process")
            .env("EDGEAGENT_PROOF_COMPONENT", component)
            .env("EDGEAGENT_NATS_URL", url)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let result = tokio::time::timeout(Duration::from_secs(15), async {
            let mut output = BufReader::new(child.stdout.take().ok_or("child stdout missing")?);
            let mut ready = String::new();
            output.read_line(&mut ready).await?;
            if ready.trim() != "ready" {
                return Err::<String, TestError>("worker failed before readiness".into());
            }
            let mut input = child.stdin.take().ok_or("child stdin missing")?;
            input.write_all(if commit { b"c" } else { b"r" }).await?;
            drop(input);
            let mut outcome = String::new();
            output.read_line(&mut outcome).await?;
            if !child.wait().await?.success() {
                return Err("worker returned unresolved failure".into());
            }
            Ok(outcome.trim().to_owned())
        })
        .await;
        // Reap on every ordinary error/deadline; kill-on-drop covers cancellation.
        if !matches!(result, Ok(Ok(_))) && child.try_wait()?.is_none() {
            tokio::time::timeout(DEADLINE, child.kill()).await??;
        }
        result?
    }

    async fn close(&mut self) -> TestResult {
        sqlx::raw_sql("SET search_path TO public")
            .execute(&mut self.admin)
            .await?;
        for schema in self.schemas.iter().rev() {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
                .execute(&mut self.admin)
                .await?;
        }
        for role in self.roles.iter().rev() {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP ROLE {role}")))
                .execute(&mut self.admin)
                .await?;
        }
        Ok(())
    }
}

pub async fn with_database<F>(body: F) -> TestResult
where
    F: for<'a> FnOnce(
        &'a mut Database,
    ) -> std::pin::Pin<Box<dyn Future<Output = TestResult> + Send + 'a>>,
{
    let mut database = Database::new().await?;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(90), async {
        database.prepare().await?;
        body(&mut database).await
    }))
    .catch_unwind()
    .await;
    let cleanup = tokio::time::timeout(DEADLINE, database.close()).await;
    match outcome {
        Ok(Ok(Ok(()))) => cleanup??,
        Ok(Ok(Err(error))) => {
            if !matches!(cleanup, Ok(Ok(()))) {
                eprintln!("owned database cleanup also failed");
            }
            return Err(error);
        }
        Ok(Err(error)) => {
            if !matches!(cleanup, Ok(Ok(()))) {
                eprintln!("owned database cleanup also failed after deadline");
            }
            return Err(error.into());
        }
        Err(panic) => {
            if !matches!(cleanup, Ok(Ok(()))) {
                eprintln!("owned database cleanup also failed after panic");
            }
            std::panic::resume_unwind(panic);
        }
    }
    Ok(())
}
