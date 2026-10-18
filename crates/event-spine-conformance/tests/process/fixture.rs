use std::{
    error::Error,
    future::Future,
    panic::AssertUnwindSafe,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use edgeagent_inbox_postgres::PostgresInbox;
use edgeagent_outbox_postgres::PostgresOutbox;
use futures_util::FutureExt;
use sqlx::{ConnectOptions, Connection, PgConnection, postgres::PgConnectOptions};
#[path = "worker.rs"]
mod worker;
pub use worker::Worker;

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
                        "conformance_probe, conformance_command, edgeagent_message_inbox, edgeagent_message_quarantine"
                    };
                    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                        "GRANT SELECT, INSERT ON {tables} TO {role}"
                    )))
                    .execute(&mut self.admin)
                    .await?;
                    if schema == self.consumer {
                        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                            "GRANT UPDATE ON conformance_probe TO {role}; GRANT UPDATE (last_delivery_attempt, last_observed_at) ON edgeagent_message_quarantine TO {role}"
                        )))
                        .execute(&mut self.admin)
                        .await?;
                    }
                }
            }
            let operator = format!("{schema}_o");
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "CREATE ROLE {operator} LOGIN PASSWORD 'fixture-only'"
            )))
            .execute(&mut self.admin)
            .await?;
            self.roles.push(operator.clone());
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "GRANT USAGE ON SCHEMA {schema} TO {operator};
                 GRANT SELECT ON edgeagent_message_outbox TO {operator};
                 GRANT SELECT, INSERT ON edgeagent_message_outbox_replay_audit TO {operator};
                 GRANT UPDATE (available_at, attempt_count, lease_owner, lease_expires_at, last_failure_code, quarantined_at, quarantine_reason) ON edgeagent_message_outbox TO {operator}"
            ))).execute(&mut self.admin).await?;
            if schema == self.consumer {
                let dispatcher = format!("{schema}_d");
                sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                    "CREATE ROLE {dispatcher} LOGIN PASSWORD 'fixture-only'"
                )))
                .execute(&mut self.admin)
                .await?;
                self.roles.push(dispatcher.clone());
                sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                    "GRANT SELECT ON edgeagent_message_quarantine, conformance_replay_receipt TO {operator};
                     GRANT UPDATE (last_observed_at) ON edgeagent_message_quarantine TO {operator};
                     GRANT SELECT, INSERT ON edgeagent_message_quarantine_replay_audit, conformance_replay_dispatch TO {operator};
                     GRANT USAGE ON SCHEMA {schema} TO {dispatcher};
                     GRANT SELECT ON conformance_replay_dispatch, edgeagent_message_quarantine_replay_audit, edgeagent_message_quarantine TO {dispatcher};
                     GRANT UPDATE (attempt_count, generation, lease_owner, lease_expires_at, published_at) ON conformance_replay_dispatch TO {dispatcher};
                     GRANT INSERT ON conformance_replay_receipt TO {dispatcher}"
                ))).execute(&mut self.admin).await?;
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

    pub async fn inspector(&self) -> Result<PgConnection, TestError> {
        Ok(PgConnection::connect_with(&self.options).await?)
    }

    pub fn replay_options(&self, consumer: bool, dispatcher: bool) -> PgConnectOptions {
        let schema = if consumer {
            &self.consumer
        } else {
            &self.producer
        };
        self.options
            .clone()
            .username(&format!("{schema}_{}", if dispatcher { "d" } else { "o" }))
            .password("fixture-only")
    }

    pub async fn replay_worker(
        &self,
        consumer: bool,
        dispatcher: bool,
        component: &str,
        url: &str,
    ) -> Result<Worker, TestError> {
        let schema = if consumer {
            &self.consumer
        } else {
            &self.producer
        };
        Worker::spawn(
            if dispatcher {
                "replay-dispatch"
            } else {
                "replay-authorize"
            },
            self.replay_options(consumer, dispatcher)
                .to_url_lossy()
                .as_str(),
            schema,
            component,
            url,
        )
        .await
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
        let mut worker = self.spawn(role, consumer, url).await?;
        worker.begin(if commit { b'c' } else { b'r' }, &[]).await?;
        worker.finish().await
    }

    pub async fn spawn(&self, role: &str, consumer: bool, url: &str) -> Result<Worker, TestError> {
        self.spawn_observed(role, consumer, url, None).await
    }

    pub async fn spawn_observed(
        &self,
        role: &str,
        consumer: bool,
        url: &str,
        telemetry: Option<&str>,
    ) -> Result<Worker, TestError> {
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
        Worker::spawn_observed(role, &database_url, schema, component, url, telemetry).await
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
