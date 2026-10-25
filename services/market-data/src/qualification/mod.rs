//! Finite explicitly selected qualification. Default/offline modes never enter this module.
mod input;
mod policy;
mod precise;
#[cfg(test)]
mod tests;
mod worker;
use BudgetError as Error;
use BudgetErrorKind as K;
use edgeagent_contracts::{MessageEnvelope, evidence::*};
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_object_store::ImmutableEvidenceStore;
use edgeagent_market_data_postgres::PostgresAcquisitionLedger as Ledger;
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
type Result<T = ()> = std::result::Result<T, Error>;
fn invalid<T>(_: T) -> Error {
    Error::new(K::Invalid)
}
fn denied<T>(_: T) -> Error {
    Error::new(K::Denied)
}
fn now() -> Result<UtcInstant> {
    UtcInstant::from_unix_millis(
        i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(invalid)?
                .as_millis(),
        )
        .map_err(invalid)?,
    )
    .map_err(invalid)
}
fn env(name: &'static str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::with_source(K::Invalid, MissingConfiguration(name)))
}
fn external_intent(mock_configured: bool, opt_in: &str, p: &QualificationProfile) -> Result {
    if mock_configured || opt_in != p.digest().as_str() {
        return Err(denied(()));
    }
    Ok(())
}
fn loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|a| a.is_loopback())
}
fn database_options(value: &str) -> Result<sqlx::postgres::PgConnectOptions> {
    let options: sqlx::postgres::PgConnectOptions = value.parse().map_err(invalid)?;
    if !loopback(options.get_host())
        && !matches!(
            options.get_ssl_mode(),
            sqlx::postgres::PgSslMode::VerifyFull
        )
    {
        return Err(denied(()));
    }
    Ok(options)
}
fn broker_url(value: String) -> Result<String> {
    let parsed = url::Url::parse(&value).map_err(invalid)?;
    if !matches!(parsed.scheme(), "nats" | "tls")
        || parsed
            .host_str()
            .is_none_or(|h| !loopback(h) && parsed.scheme() != "tls")
    {
        return Err(denied(()));
    }
    Ok(value)
}
fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(Error::storage)?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(Error::storage)?;
    if u64::try_from(bytes.len()).map_err(invalid)? > limit {
        return Err(invalid(()));
    }
    Ok(bytes)
}
fn profile(args: &[String]) -> Result<QualificationProfile> {
    let [_, path, hash] = args else {
        return Err(invalid(()));
    };
    QualificationProfile::decode(
        &read(Path::new(path), 8192)?,
        &EvidenceDigest::new(hash).map_err(invalid)?,
    )
}
pub(super) fn run(args: &[String]) -> std::process::ExitCode {
    let result = (|| -> Result {
        // Explicit mode precedes infrastructure or secret access. No implicit USE_MOCK=0 switch.
        let p = profile(args)?;
        if !matches!(
            args.first().map(String::as_str),
            Some(
                "qualification-preflight"
                    | "qualification-worker"
                    | "qualification-maintain"
                    | "qualification-relay"
            )
        ) {
            return Err(invalid(()));
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(Error::storage)?;
        let result = rt.block_on(execute(&args[0], p));
        rt.shutdown_timeout(Duration::from_secs(15));
        result
    })();
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("qualification_failed={:?}", e.kind());
            if let Some(missing) = std::error::Error::source(&e)
                .and_then(|cause| cause.downcast_ref::<MissingConfiguration>())
            {
                eprintln!("{missing}");
            }
            std::process::ExitCode::from(2)
        }
    }
}
#[derive(Debug)]
struct MissingConfiguration(&'static str);
impl std::fmt::Display for MissingConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "required configuration missing or invalid: {}", self.0)
    }
}
impl std::error::Error for MissingConfiguration {}
async fn execute(command: &str, p: QualificationProfile) -> Result {
    let schema = env("EDGEAGENT_QUALIFICATION_SCHEMA")?;
    if schema.len() > 63
        || schema.is_empty()
        || !schema
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(invalid(()));
    }
    let options = database_options(&env("EDGEAGENT_QUALIFICATION_DATABASE_URL")?)?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options.options([
            ("search_path", schema.as_str()),
            ("statement_timeout", "5000"),
            ("lock_timeout", "1000"),
        ]))
        .await
        .map_err(Error::storage)?;
    let ledger = Ledger::for_qualification(pool.clone(), p.clone());
    let result = async {
        if command == "qualification-relay" {
            return relay(&pool).await;
        }
        if command == "qualification-maintain" {
            // Separate owner LOGIN. No worker callback can create this authority.
            let owner = Ledger::new(pool.clone());
            owner.retire_due_evidence(16).await?;
            let store = storage()?;
            let batch = owner.pending_cleanup(16).await?;
            let mut complete = 0;
            let mut first_error = None;
            for item in batch {
                let result: Result = async {
                    let authority = owner.qualification_cleanup(&item.request).await?;
                    authority.confirm_writes_quiescent(&item.request)?;
                    for object in &item.objects {
                        authority.authorize_deletion(&item.request, object)?;
                        store
                            .delete(&item.request, object, &authority)
                            .await
                            .map_err(Error::storage)?;
                    }
                    owner.complete_cleanup(&item.request).await
                }
                .await;
                match result {
                    Ok(()) => complete += 1,
                    Err(e) => {
                        first_error.get_or_insert(e);
                    }
                }
            }
            println!("cleanup_confirmed={complete}");
            return first_error.map_or(Ok(()), Err);
        }
        if command == "qualification-worker" {
            return worker::run(&pool, &ledger, &p).await;
        }
        let (record, _) = ledger.load(&p.data().admission).await?;
        let data = ledger.qualification_input().await?;
        let input = input::Input::decode(&data, &record, &p)?;
        let snapshot = ledger.qualification_snapshot().await?;
        let report = preflight_qualification(
            &p,
            &record,
            input.catalogs(),
            snapshot.at(),
            &Cancellation::default(),
            &snapshot,
        )?;
        if command == "qualification-preflight" {
            println!("{}", report.status());
            return Ok(());
        }
        Err(invalid(()))
    }
    .await;
    let closed = tokio::time::timeout(Duration::from_secs(5), pool.close())
        .await
        .map_err(Error::storage);
    result.and(closed)
}
async fn relay(pool: &sqlx::PgPool) -> Result {
    let client = tokio::time::timeout(
        Duration::from_secs(5),
        async_nats::connect(broker_url(env("EDGEAGENT_QUALIFICATION_NATS_URL")?)?),
    )
    .await
    .map_err(Error::storage)?
    .map_err(Error::storage)?;
    let result = async {
        let publisher = edgeagent_messaging_nats::JetStreamPublisher::from_context(
            async_nats::jetstream::new(client.clone()),
        );
        let mut connection = pool.acquire().await.map_err(Error::storage)?;
        let mut store = edgeagent_outbox_postgres::PostgresOutboxRelay::new(&mut connection);
        let policy = edgeagent_outbox_relay::RelayPolicy::new(
            "qualification-market-data",
            edgeagent_messaging::LeaseDuration::new(Duration::from_secs(30)).map_err(invalid)?,
            edgeagent_outbox_relay::MaxAttempts::new(3).map_err(invalid)?,
            edgeagent_outbox_relay::RetryBackoff::new(
                Duration::from_secs(1),
                Duration::from_secs(60),
            )
            .map_err(invalid)?,
        )
        .map_err(invalid)?;
        let registry = registry().map_err(invalid)?;
        for _ in 0..16 {
            let outcome = tokio::time::timeout(
                Duration::from_secs(15),
                edgeagent_outbox_relay::relay_once(&mut store, &registry, &publisher, &policy),
            )
            .await
            .map_err(Error::storage)?
            .map_err(Error::storage)?;
            if matches!(outcome, edgeagent_outbox_relay::RelayOutcome::Idle) {
                break;
            }
            println!("relay_progress=1");
        }
        Ok(())
    }
    .await;
    let drain = tokio::time::timeout(Duration::from_secs(5), client.drain())
        .await
        .map_err(Error::storage)?
        .map_err(Error::storage);
    result.and(drain)
}
fn storage() -> Result<ImmutableEvidenceStore> {
    // The deployment independently provides a private encrypted persistent volume.
    // This command never creates roots, migrations or cloud credentials.
    let root = env("EDGEAGENT_QUALIFICATION_OBJECT_ROOT")?;
    let path = Path::new(&root);
    if !path.is_absolute() || !path.is_dir() {
        return Err(invalid(()));
    }
    let backend =
        object_store::local::LocalFileSystem::new_with_prefix(path).map_err(Error::storage)?;
    ImmutableEvidenceStore::new(Arc::new(backend), Duration::from_secs(15)).map_err(Error::storage)
}
