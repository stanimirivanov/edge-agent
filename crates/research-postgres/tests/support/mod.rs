#![allow(
    clippy::disallowed_methods,
    reason = "Test-only isolated schema administration and fault injection."
)]
use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    research::{ResearchRecord, ResearchRequest},
};
use edgeagent_market_data_domain::{InstrumentId, ListingId, SessionDate, UsdPrice};
use edgeagent_research_application::publication::{EVALUATE, PreparedCompletion};
use edgeagent_research_domain::*;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    str::FromStr,
    sync::atomic::{AtomicUsize, Ordering},
};
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const TIME: &str = "2026-10-29T00:00:00Z";
pub fn prepared(
    scope: &str,
    id: &str,
    delivery: &str,
    last: i128,
    config: u8,
) -> Result<PreparedCompletion> {
    build(scope, id, delivery, last, config, true, false)
}
pub fn abstained(scope: &str, id: &str, delivery: &str) -> Result<PreparedCompletion> {
    build(scope, id, delivery, 100, 3, false, false)
}
pub fn policy_denied(scope: &str, id: &str, delivery: &str) -> Result<PreparedCompletion> {
    build(scope, id, delivery, 110, 3, true, true)
}
fn build(
    scope: &str,
    id: &str,
    delivery: &str,
    last: i128,
    config: u8,
    with_evidence: bool,
    deny_liquidity: bool,
) -> Result<PreparedCompletion> {
    let selected = SelectedInstrument {
        instrument: InstrumentId::new(1)?,
        listing: ListingId::new(1)?,
        mapping: [1; 32],
    };
    let sessions = vec![
        SessionDate::new(2026, 10, 1)?,
        SessionDate::new(2026, 10, 2)?,
    ];
    let base = execution(config)?;
    let selection = if deny_liquidity {
        let mut wire = edgeagent_contracts::strategy::ExecutionConfiguration::decode(base.bytes())?;
        let edgeagent_contracts::strategy::ExecutionConfiguration::V1 { bundle, .. } = &mut wire;
        let edgeagent_contracts::strategy::Configuration::V1 { policy, .. } = bundle;
        policy.min_daily_turnover_cents = u64::MAX;
        edgeagent_research_application::execution::Selection::decode(&wire.canonical_bytes()?)?
    } else {
        base
    };
    let profile = selection.profile(scope);
    let request = ResearchRequest {
        scope: scope.into(),
        request_id: id.into(),
        implementation: "02".repeat(32),
        configuration: profile.configuration.clone(),
        decision_at_ms: 100,
        lookback_sessions: 1,
        instruments: vec![format!("{:032x}", 1)],
    };
    let policy = EvidencePolicy::new(
        PolicyBinding {
            decision_at_ms: 100,
            max_age_ms: 10,
            implementation: [2; 32],
            configuration: decode_digest(&profile.configuration)?,
            calendar: [4; 32],
        },
        vec![selected],
        sessions.clone(),
        1,
    )?;
    let data = [FrozenSeries {
        selected,
        evidence: [5; 32],
        implementation: [2; 32],
        configuration: decode_digest(&profile.configuration)?,
        calendar: [4; 32],
        known_at_ms: 90,
        rights_until_ms: 101,
        entitled: true,
        quality_accepted: true,
        basis: AdjustmentBasis::Raw,
        closes: sessions
            .into_iter()
            .enumerate()
            .map(|(i, session)| {
                Ok(FrozenClose {
                    session,
                    close: UsdPrice::new(if i == 0 { 100 } else { last }, 0)?,
                    event_at_ms: 80 + i as i64 * 10,
                    observed_at_ms: 90,
                    available_at_ms: 100,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    }];
    let record = ResearchRecord::Request(request.clone());
    let partition = format!("research-request/{scope}/{id}");
    let command = MessageEnvelope::from_payload(
        MessageMetadata {
            id: delivery.into(),
            source: Component::Gateway.source_uri().into(),
            message_type: EVALUATE.message_type().into(),
            subject: partition.clone(),
            time: TIME.into(),
            data_schema: EVALUATE.data_schema().into(),
            correlation_id: "correlation-01".into(),
            causation_id: "cause-01".into(),
            idempotency_key: record.digest()?,
            partition_key: partition,
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
            trace_state: None,
        },
        &record,
    )?;
    let instrument = selected.instrument;
    let evidence = [5; 32];
    let volumes = [edgeagent_research_domain::liquidity::Input {
        instrument,
        evidence,
        rows: data[0]
            .closes
            .iter()
            .map(|c| {
                Ok(edgeagent_research_domain::liquidity::Volume {
                    session: c.session,
                    shares: edgeagent_market_data_domain::ShareVolume::new(1, 0)?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    }];
    let opens = [edgeagent_research_domain::market_constraints::Input {
        instrument,
        evidence,
        rows: data[0]
            .closes
            .iter()
            .map(|c| {
                Ok(edgeagent_research_domain::market_constraints::Open {
                    session: c.session,
                    price: UsdPrice::new(100, 0)?,
                    action: false,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    }];
    let context = [edgeagent_research_domain::context_policy::Input {
        instrument,
        evidence,
        coverage: edgeagent_research_domain::context_policy::Coverage::Complete {
            source: [7; 32],
            known_at_ms: 90,
            interval: [0, 101],
            events: vec![],
        },
    }];
    let outcome = selection.validate(
        request,
        &policy,
        if with_evidence { &data } else { &[] },
        101,
        edgeagent_research_application::governed::ContextInputs {
            constraints: edgeagent_research_application::governed::ConstraintInputs {
                volumes: &volumes,
                opens: &opens,
            },
            context: &context,
        },
    )?;
    Ok(PreparedCompletion::from_execution(command, &outcome, TIME)?)
}
pub fn execution(config: u8) -> Result<edgeagent_research_application::execution::Selection> {
    let mut value: edgeagent_contracts::strategy::ExecutionConfiguration =
        edgeagent_contracts::strategy::ExecutionConfiguration::decode(include_bytes!(
            "../../../contracts/fixtures/strategy/execution-v1.json"
        ))?;
    let edgeagent_contracts::strategy::ExecutionConfiguration::V1 { bundle, .. } = &mut value;
    let edgeagent_contracts::strategy::Configuration::V1 { policy, .. } = bundle;
    policy.max_horizon_sessions = u16::from(config);
    Ok(edgeagent_research_application::execution::Selection::decode(&value.canonical_bytes()?)?)
}
fn decode_digest(text: &str) -> Result<[u8; 32]> {
    let mut result = [0; 32];
    for (i, b) in result.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)?;
    }
    Ok(result)
}

pub struct Harness {
    pub pool: PgPool,
    root: PgPool,
    options: PgConnectOptions,
    pub key: i64,
    schema: String,
}
impl Harness {
    pub async fn approved() -> Result<Self> {
        let h = Self::new().await?;
        let store = edgeagent_research_postgres::PostgresResearchStore::new(h.pool.clone());
        for scope in ["scope", "another-scope", "foreign"] {
            for config in [3, 9] {
                let profile = execution(config)?.profile(scope);
                store.govern(&profile, 1).await?;
                store.govern(&profile, 2).await?;
            }
        }
        Ok(h)
    }
    pub async fn new() -> Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let options = PgConnectOptions::from_str(&std::env::var("EDGEAGENT_POSTGRES_URL")?)?;
        if options.get_host() != "localhost"
            && !options
                .get_host()
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        {
            return Err("test database must be loopback".into());
        }
        let root = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await?;
        let counter = NEXT.fetch_add(1, Ordering::SeqCst);
        let schema = format!("research_{}_{}", std::process::id(), counter);
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&root)
            .await?;
        let options = options.options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .connect_with(options.clone())
            .await?;
        for sql in edgeagent_inbox_postgres::PostgresInbox::MIGRATIONS
            .into_iter()
            .chain(edgeagent_outbox_postgres::PostgresOutbox::MIGRATIONS)
            .chain([
                edgeagent_research_postgres::MIGRATION,
                edgeagent_research_postgres::lifecycle::MIGRATION,
                edgeagent_research_postgres::governance::MIGRATION,
                edgeagent_research_postgres::DECISION_MIGRATION,
            ])
        {
            sqlx::raw_sql(sql).execute(&pool).await?;
        }
        Ok(Self {
            pool,
            root,
            options,
            key: i64::from(std::process::id()) * 1000 + counter as i64,
            schema,
        })
    }
    pub async fn reconnect(&self) -> Result<PgPool> {
        Ok(PgPoolOptions::new()
            .max_connections(1)
            .connect_with(self.options.clone())
            .await?)
    }
    // Original terminal/lifecycle publication census; companion proofs inspect all four tables explicitly.
    pub async fn counts(&self) -> Result<(i64, i64, i64)> {
        Ok((
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_research_completions")
                .fetch_one(&self.pool)
                .await?,
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_inbox")
                .fetch_one(&self.pool)
                .await?,
            sqlx::query_scalar("SELECT count(*) FROM edgeagent_message_outbox WHERE message_type<>'com.edgeagent.research.decision-recorded.v1'")
                .fetch_one(&self.pool)
                .await?,
        ))
    }
    pub async fn finish(
        self,
        result: std::result::Result<Result, Box<dyn std::any::Any + Send>>,
    ) -> Result {
        self.pool.close().await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.root)
        .await?;
        self.root.close().await;
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}
