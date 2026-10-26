use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    error::Error as _,
    num::NonZeroU32,
    str::FromStr,
    sync::atomic::{AtomicUsize, Ordering},
};
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub fn kind<T>(r: std::result::Result<T, BudgetError>, expected: BudgetErrorKind) -> Result {
    assert_eq!(r.err().ok_or("expected budget failure")?.kind(), expected);
    Ok(())
}
#[allow(
    dead_code,
    reason = "Shared by contended admission and terminal proofs only."
)]
pub fn is_lock_timeout(error: &BudgetError) -> bool {
    error.kind() == BudgetErrorKind::Storage
        && error
            .source()
            .and_then(|cause| cause.downcast_ref::<sqlx::Error>())
            .and_then(sqlx::Error::as_database_error)
            .is_some_and(|cause| cause.code().as_deref() == Some("55P03"))
}
pub struct Harness {
    pub pool: PgPool,
    root: PgPool,
    options: PgConnectOptions,
    schema: String,
    pub now: i64,
}
impl Harness {
    pub async fn new() -> Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let url = std::env::var("EDGEAGENT_POSTGRES_URL")?;
        let options = PgConnectOptions::from_str(&url)?;
        let root = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await?;
        let schema = format!(
            "acquisition_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        // Identifier consists only of the fixed prefix and process/atomic integers.
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&root)
            .await?;
        let options = options.options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options.clone())
            .await?;
        for migration in edgeagent_market_data_postgres::MIGRATIONS {
            sqlx::raw_sql(migration).execute(&pool).await?;
        }
        sqlx::raw_sql(edgeagent_inbox_postgres::PostgresInbox::MIGRATION_SQL)
            .execute(&pool)
            .await?;
        sqlx::raw_sql(edgeagent_outbox_postgres::PostgresOutbox::MIGRATION_SQL)
            .execute(&pool)
            .await?;
        let now =
            sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint")
                .fetch_one(&pool)
                .await?;
        Ok(Self {
            pool,
            root,
            options,
            schema,
            now,
        })
    }
    pub async fn reconnect(&self) -> Result<PgPool> {
        Ok(PgPoolOptions::new()
            .max_connections(2)
            .connect_with(self.options.clone())
            .await?)
    }
    #[allow(
        dead_code,
        reason = "Used by qualification role proofs; other targets share this fixture."
    )]
    pub async fn worker(&self) -> Result<(PgPool, String)> {
        static NEXT_WORKER: AtomicUsize = AtomicUsize::new(0);
        let role = format!(
            "qualification_worker_{}_{}",
            std::process::id(),
            NEXT_WORKER.fetch_add(1, Ordering::SeqCst)
        );
        // Invented, issue-owned LOGIN role: test actual authentication, not SET ROLE
        // on an administrator session that could RESET ROLE back to the owner.
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE ROLE {role} LOGIN PASSWORD '{role}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOINHERIT;
            GRANT USAGE ON SCHEMA {schema} TO {role};
            GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA {schema} TO {role};
            GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA {schema} TO {role};
            REVOKE ALL ON {schema}.edgeagent_qualification_grants FROM {role};
            GRANT SELECT ON {schema}.edgeagent_qualification_grants TO {role};
            REVOKE ALL ON {schema}.edgeagent_qualification_inputs,{schema}.edgeagent_qualification_cleanup FROM {role};
            GRANT SELECT ON {schema}.edgeagent_qualification_inputs,{schema}.edgeagent_qualification_cleanup TO {role};
            REVOKE ALL ON {schema}.edgeagent_stream_accounts,{schema}.edgeagent_stream_admissions FROM {role};
            GRANT SELECT ON {schema}.edgeagent_stream_accounts,{schema}.edgeagent_stream_admissions TO {role};
            GRANT UPDATE(charged_starts,generation,last_start_ms) ON {schema}.edgeagent_stream_accounts TO {role};
            REVOKE INSERT,DELETE,TRUNCATE ON {schema}.edgeagent_stream_sessions FROM {role};
            GRANT EXECUTE ON FUNCTION {schema}.edgeagent_lock_stream_admission(TEXT),{schema}.edgeagent_stream_worker_allowed() TO {role};
            GRANT EXECUTE ON FUNCTION {schema}.edgeagent_qualification_input_role() TO {role};
            GRANT EXECUTE ON FUNCTION {schema}.edgeagent_qualification_input(TEXT) TO {role};
            GRANT EXECUTE ON FUNCTION {schema}.edgeagent_lock_qualification_grant(TEXT),{schema}.edgeagent_qualification_role() TO {role};",schema=self.schema)))
            .execute(&self.root).await?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(self.options.clone().username(&role).password(&role))
            .await?;
        Ok((pool, role))
    }
    #[allow(
        dead_code,
        reason = "Used by qualification role proofs; other targets share this fixture."
    )]
    pub async fn drop_worker(&self, pool: PgPool, role: &str) -> Result {
        pool.close().await;
        if !role.starts_with("qualification_worker_")
            || !role.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("invalid owned worker role".into());
        }
        let reviewer = format!("{role}_reviewer");
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=$1)")
                .bind(&reviewer)
                .fetch_one(&self.root)
                .await?;
        if exists {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP OWNED BY {reviewer}; DROP ROLE {reviewer};"
            )))
            .execute(&self.root)
            .await?;
        }
        // Only the unique role created above owns these grants; no shared role is touched.
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP OWNED BY {role}; DROP ROLE {role};"
        )))
        .execute(&self.root)
        .await?;
        Ok(())
    }
    pub async fn age_rate_history(&self) -> Result {
        // Advance the persisted rate fixture, not production time or command counters.
        let bytes: Vec<u8> =
            sqlx::query_scalar("SELECT budget FROM edgeagent_acquisition_accounts")
                .fetch_one(&self.pool)
                .await?;
        let mut data: serde_json::Value = serde_json::from_slice(&bytes)?;
        data["starts"] = serde_json::json!([]);
        sqlx::query("UPDATE edgeagent_acquisition_accounts SET budget=$1")
            .bind(serde_json::to_vec(&data)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn configure_credits(&self) -> Result {
        edgeagent_market_data_postgres::PostgresAcquisitionLedger::new(self.pool.clone())
            .configure_daily_credits(
                ProviderAccountId::new(1)?,
                DailyCreditPolicy::new(
                    1,
                    100,
                    self.now + 86_400_000,
                    &[(ReferenceId::new(1)?, 1)],
                )?,
                0,
            )
            .await?;
        Ok(())
    }
    pub async fn finish(self, result: std::thread::Result<Result>) -> Result {
        self.pool.close().await;
        // Same owner-generated identifier used for CREATE; never user-supplied SQL.
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.root)
        .await?;
        self.root.close().await;
        match result {
            Ok(r) => r,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}
pub struct Case {
    pub request: AcquisitionRequest,
    rights: ReferenceHistory<RightsRecord>,
    providers: ReferenceHistory<ProviderReview>,
}
impl Case {
    pub fn new(ms: i64, id: u128) -> Result<Self> {
        Self::with_precision(ms, id, TimestampPrecision::Milliseconds)
    }
    pub fn with_precision(ms: i64, id: u128, precision: TimestampPrecision) -> Result<Self> {
        let at = |value| UtcInstant::from_unix_millis(value);
        let scope = AccessScope {
            principal: PrincipalId::new(1)?,
            account: ProviderAccountId::new(1)?,
            source: SourceId::new(1)?,
            instrument: InstrumentId::new(1)?,
            endpoint: ReferenceId::new(1)?,
            audience: AudienceId::new(1)?,
            storage: StorageScopeId::new(1)?,
        };
        let contract = EvidenceContract::new(
            EvidenceKind::RawBar(BarCoverage::ProviderDay),
            MarketScope::Consolidated,
            SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
            precision,
        )?;
        let interval = EffectiveInterval::new(at(ms - 1000)?, at(ms + 86_400_000)?)?;
        let right = RightsRecord {
            scope,
            contract,
            agreement: ReferenceId::new(1)?,
            effective: interval,
            permissions: UsePermissions {
                automated: Permission::Allowed,
                derived: Permission::Allowed,
                canonical_storage: Permission::Allowed,
                audience: Permission::Allowed,
            },
            raw_storage: Permission::Denied,
            canonical_retention_ms: NonZeroU32::new(600000).ok_or("nonzero")?,
            raw_retention_ms: None,
            delete_by: at(ms + 600000)?,
            embargo_ms: 0,
            status: ReviewStatus::Active,
        };
        let review = ProviderReview {
            scope,
            capability: ProviderCapability::new(
                scope.source,
                contract,
                DeliveryCapability::Snapshots,
                KnowledgeCapability::CaptureOnly,
            ),
            effective: interval,
            status: ReviewStatus::Active,
        };
        let rights = ReferenceHistory::new(vec![ReferenceVersion::new(
            ReferenceId::new(1)?,
            ReferenceRevision::new(1)?,
            scope.source,
            at(ms - 1000)?,
            [7; 32],
            right,
        )])?;
        let providers = ReferenceHistory::new(vec![ReferenceVersion::new(
            ReferenceId::new(1)?,
            ReferenceRevision::new(1)?,
            scope.source,
            at(ms - 1000)?,
            [8; 32],
            review,
        )])?;
        let nz = |n| NonZeroU32::new(n).ok_or("nonzero");
        let request = AcquisitionRequest {
            id: AcquisitionId::new(id)?,
            scope,
            contract,
            range: DateRange::new(SessionDate::new(1970, 1, 1)?, SessionDate::new(1970, 1, 2)?)?,
            knowledge: KnowledgeMode::Capture,
            admitted_at: at(ms)?,
            deadline: at(ms + 300000)?,
            budget: RequestBudget {
                connect_ms: nz(5000)?,
                request_ms: nz(15000)?,
                calls: nz(12)?,
                pages: nz(4)?,
                response_bytes: nz(1_048_576)?,
                command_bytes: nz(8_388_608)?,
            },
            raw: RawRetention::Discard,
        };
        Ok(Self {
            request,
            rights,
            providers,
        })
    }
    pub fn catalogs(&self) -> AuthorityCatalogs<'_> {
        AuthorityCatalogs {
            rights: &self.rights,
            providers: &self.providers,
        }
    }
    pub fn record(&self) -> Result<AdmissionRecord> {
        self.record_with_spacing(15000)
    }
    pub fn record_with_spacing(&self, spacing_ms: u32) -> Result<AdmissionRecord> {
        let cancel = Cancellation::default();
        let admitted = self
            .catalogs()
            .admit(&self.request, self.request.admitted_at, &cancel)?;
        Ok(AdmissionRecord::new(
            &admitted,
            AcquisitionLimits {
                attempts: 3,
                spacing_ms,
                starts_per_minute: 4,
            },
            ListingId::new(1)?,
            DailyQualityPolicy::new(TransformationRevision::new(1)?, 60000, 1000, 100)?,
        )?)
    }
}
