//! Finite, local-infrastructure, invented-data worker. Vendor activation is absent.
mod bootstrap;
mod config;
mod fixture;
mod handling;
mod policy;
use handling::handle;
#[cfg(test)]
mod tests;
use edgeagent_contracts::{MessageEnvelope, evidence::*};
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::UtcInstant;
use edgeagent_market_data_object_store::ImmutableEvidenceStore;
use edgeagent_market_data_postgres::PostgresAcquisitionLedger;
use edgeagent_messaging::{DeliveryDisposition, MessageConsumer, MessageDelivery, RetryDelay};
use edgeagent_messaging_nats::JetStreamConsumer;
use fixture::Job;
use policy::{Access, Cleanup};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn now() -> Result<UtcInstant> {
    let ms = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    Ok(UtcInstant::from_unix_millis(ms)?)
}

pub(super) fn run(args: &[String]) -> std::process::ExitCode {
    let result = (|| -> Result {
        // Mode admission precedes infrastructure/credential lookup.
        let config = config::Config::read(args)?;
        let _owner = config.lock()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let result = runtime.block_on(execute(&config));
        runtime.shutdown_timeout(Duration::from_secs(15));
        result
    })();
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if let Some(error) = error.downcast_ref::<config::ConfigError>() {
                eprintln!("{error}");
            } else {
                // Never format SQL, filesystem, broker or provider source chains.
                let mut cause = Some(error.as_ref() as &(dyn std::error::Error + 'static));
                while let Some(current) = cause {
                    if let Some(policy) = current.downcast_ref::<BudgetError>() {
                        eprintln!("failure_policy={:?}", policy.kind());
                    }
                    if let Some(sqlx::Error::Database(database)) =
                        current.downcast_ref::<sqlx::Error>()
                        && let Some(code) = database.code().filter(|code| {
                            code.len() == 5 && code.bytes().all(|v| v.is_ascii_alphanumeric())
                        })
                    {
                        eprintln!("failure_sqlstate={code}");
                    }
                    cause = current.source();
                }
                eprintln!(
                    "offline worker failed closed: dependency, policy or confirmation failure"
                );
            }
            std::process::ExitCode::from(2)
        }
    }
}

async fn execute(config: &config::Config) -> Result {
    let schema = config.schema()?;
    let options: sqlx::postgres::PgConnectOptions = config.database.parse()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options.options([
            ("search_path", schema.as_str()),
            ("statement_timeout", "5000"),
            ("lock_timeout", "1000"),
        ]))
        .await?;
    let result: Result = async {
        if config.command == "offline-init" {
            return tokio::time::timeout(Duration::from_secs(60), bootstrap::initialize(config, &pool, &schema)).await?;
        }
        let ledger = PostgresAcquisitionLedger::new(pool.clone());
        if config.command == "offline-seed" {
            let job = Job::with_recipe(config.request.ok_or("missing invented request")?, now()?.unix_millis(), config.recipe)?;
            let cancel = Cancellation::default();
            let authorized = job.catalogs().admit(job.record.request(), now()?, &cancel)?;
            ledger.admit(&job.record, &authorized).await?;
            // Reference-only command data, not a gateway identity or a publication claim.
            println!("{}", serde_json::to_string(job.record.command())?);
            return Ok(());
        }
        let backend = object_store::local::LocalFileSystem::new_with_prefix(config.root.join("objects"))?;
        let storage = ImmutableEvidenceStore::new(Arc::new(backend), Duration::from_secs(15))?;
        let mut confirmed = config.confirmed.clone();
        if config.command == "offline-maintain" {
            maintenance(&ledger, &storage, &confirmed).await?;
            return Ok(());
        }
        let client = tokio::time::timeout(Duration::from_secs(5), async_nats::connect(config.broker.as_deref().ok_or("missing broker")?)).await??;
        let context = async_nats::jetstream::new(client.clone());
        if config.command == "offline-relay" {
            let mut connection = pool.acquire().await?;
            let mut store = edgeagent_outbox_postgres::PostgresOutboxRelay::new(&mut connection);
            let publisher = edgeagent_messaging_nats::JetStreamPublisher::from_context(context);
            let policy = edgeagent_outbox_relay::RelayPolicy::new("offline-market-data",
                edgeagent_messaging::LeaseDuration::new(Duration::from_secs(30))?,
                edgeagent_outbox_relay::MaxAttempts::new(3)?,
                edgeagent_outbox_relay::RetryBackoff::new(Duration::from_secs(1), Duration::from_secs(60))?)?;
            let registry = registry()?;
            for _ in 0..config.messages {
                let outcome = tokio::time::timeout(Duration::from_secs(15), edgeagent_outbox_relay::relay_once(&mut store, &registry, &publisher, &policy)).await??;
                if matches!(outcome, edgeagent_outbox_relay::RelayOutcome::Idle) { break; }
                println!("relay_progress=1");
            }
            tokio::time::timeout(Duration::from_secs(5), client.drain()).await??;
            return Ok(());
        }
        let stream = context.get_stream(config.stream.as_deref().ok_or("missing stream")?).await?;
        let consumer: async_nats::jetstream::consumer::PullConsumer = stream.get_consumer(config.consumer.as_deref().ok_or("missing consumer")?).await?;
        let c = &consumer.cached_info().config;
        if c.filter_subject != DEFINITIONS[0].subject()? || !c.filter_subjects.is_empty()
            || c.ack_wait < Duration::from_secs(30) || !(1..=3).contains(&c.max_deliver)
            || !(1..=32).contains(&c.max_ack_pending) {
            return Err("incompatible offline consumer bounds".into());
        }
        let mut consumer = JetStreamConsumer::new(consumer).await?;
        // Missing tables, incompatible migrations or denial storage stop readiness.
        ledger.retire_due_evidence(16).await?;
        println!("readiness=ready\nprovider=synthetic-daily-v1");
        let cancel = Cancellation::default();
        let stop = stop_signal();
        tokio::pin!(stop);
        let mut handled = 0_u8;
        for _ in 0..config.messages {
            let delivery = tokio::select! {
                signal = &mut stop => { signal?; cancel.cancel(); break; },
                result = tokio::time::timeout(Duration::from_secs(30), consumer.receive()) => result??,
            };
            {
            let work = tokio::time::timeout(Duration::from_secs(300), handle(&pool, &ledger, &storage, delivery, &cancel, &mut confirmed));
            tokio::pin!(work);
            tokio::select! {
                result = &mut work => result??,
                signal = &mut stop => {
                    signal?;
                    cancel.cancel();
                    tokio::time::timeout(Duration::from_secs(15), &mut work).await???;
                }
            }
            }
            handled += 1;
            maintenance(&ledger, &storage, &confirmed).await?;
            if cancel.is_cancelled() { break; }
        }
        tokio::time::timeout(Duration::from_secs(5), client.drain()).await??;
        println!("readiness=stopped\nhandled={handled}");
        Ok(())
    }.await;
    tokio::time::timeout(Duration::from_secs(5), pool.close()).await?;
    result
}

async fn stop_signal() -> Result {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { result = tokio::signal::ctrl_c() => result?, _ = terminate.recv() => () }
    }
    Ok(())
}

async fn maintenance(
    ledger: &PostgresAcquisitionLedger,
    storage: &ImmutableEvidenceStore,
    confirmed: &BTreeSet<EvidenceId>,
) -> Result {
    let retired = ledger.retire_due_evidence(16).await?;
    let owner = Cleanup { confirmed };
    match cleanup_evidence(ledger, storage, &owner, 16).await {
        Ok(deleted) => println!("retired={retired}\ncleanup_confirmed={deleted}"),
        Err(error) if error.kind() == BudgetErrorKind::Denied => {
            println!("retired={retired}\ncleanup_waiting_for_owner=1")
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
