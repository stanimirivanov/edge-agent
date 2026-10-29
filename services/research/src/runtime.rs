//! Explicit loopback Research worker/relay. Schema and topology provisioning are separate.
use crate::loader::OwnerLoader;
use edgeagent_contracts::{MessageEnvelope, research_acquisition::DECIDED};
use edgeagent_inbox_postgres::{PostgresInbox, QuarantineEvidence};
use edgeagent_messaging::{DeliveryDisposition, MessageConsumer, MessageDelivery, RetryDelay};
use edgeagent_research_application::{
    publication::{self, PublicationErrorKind as Kind, ResearchPublication},
    worker::{self, EvidenceLoader},
};
use edgeagent_research_postgres::PostgresResearchStore;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
/// Loopback-only URL with no query/fragment routing overrides.
/// # Errors
/// Rejects unsupported/nonlocal endpoints without echoing them.
pub fn local(value: &str, schemes: &[&str]) -> std::result::Result<(), &'static str> {
    let url = url::Url::parse(value).map_err(|_| "invalid local endpoint")?;
    if !schemes.contains(&url.scheme())
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.host_str().is_some_and(|s| {
            s == "localhost"
                || s.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        })
    {
        return Err("loopback endpoint required");
    }
    Ok(())
}
fn required(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing research configuration".into())
}
fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn time() -> Result<String> {
    let ms = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    Ok(chrono::DateTime::from_timestamp_millis(ms)
        .ok_or("invalid research clock")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
/// Bounded serving commands; descriptor behavior remains in the binary.
/// # Errors
/// No error details are printed by the composition root.
pub fn run(args: &[String]) -> Result {
    if args.len() != 1
        || !matches!(
            args[0].as_str(),
            "offline-once" | "offline-worker" | "offline-relay" | "offline-lifecycle"
        )
    {
        return Err("invalid research mode".into());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let result = runtime.block_on(execute(&args[0]));
    runtime.shutdown_timeout(Duration::from_secs(15));
    result
}
async fn execute(mode: &str) -> Result {
    let database = required("EDGEAGENT_POSTGRES_URL")?;
    local(&database, &["postgres", "postgresql"])?;
    let schema = required("EDGEAGENT_RESEARCH_SCHEMA")?;
    if !schema.starts_with("research_")
        || schema.len() > 63
        || !schema
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err("invalid private research schema".into());
    }
    let scope = required("EDGEAGENT_RESEARCH_SCOPE")?;
    if !token(&scope) {
        return Err("invalid research scope".into());
    }
    let options: sqlx::postgres::PgConnectOptions = database.parse()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options.options([
            ("search_path", schema.as_str()),
            ("statement_timeout", "15000"),
            ("lock_timeout", "5000"),
        ]))
        .await?;
    let result:Result=async {
        // Compilation/readiness requires the explicitly prepared private completion schema.
        sqlx::query!("SELECT request_id FROM edgeagent_research_completions LIMIT 0").fetch_all(&pool).await?;
        let store=PostgresResearchStore::new(pool.clone());
        if mode=="offline-lifecycle" {
            use std::io::{Read as _,Write as _};let mut bytes=Vec::new();std::io::stdin().take(131_073).read_to_end(&mut bytes)?;
            let edgeagent_contracts::research::ResearchRecord::Lifecycle(event)=edgeagent_contracts::research::ResearchRecord::decode(&bytes)? else {return Err("expected lifecycle owner command".into());};
            let event=tokio::time::timeout(Duration::from_secs(25),store.append_lifecycle(&scope,event,&time()?)).await??;
            std::io::stdout().write_all(&event.to_json()?)?;return Ok(());
        }
        if mode=="offline-once" {
            use std::io::{Read as _,Write as _};let mut bytes=Vec::new();std::io::stdin().take(262_145).read_to_end(&mut bytes)?;
            let delivery=MessageEnvelope::from_json(&bytes)?;
            if publication::evaluation_request(&delivery)?.scope!=scope {return Err("foreign research scope".into());}
            let loader=OwnerLoader::from_environment()?;
            let receipt=tokio::time::timeout(Duration::from_secs(25),worker::process(&store,&loader,&delivery,&time()?)).await??;
            std::io::stdout().write_all(receipt.bytes())?;return Ok(());
        }
        let broker=required("EDGEAGENT_NATS_URL")?;local(&broker,&["nats","tls"])?;
        let maximum=required("EDGEAGENT_RESEARCH_MAX_MESSAGES")?.parse::<u8>()?;if !(1..=16).contains(&maximum) {return Err("invalid research work bound".into());}
        let client=tokio::time::timeout(Duration::from_secs(5),async_nats::connect(broker)).await??;
        let context=async_nats::jetstream::new(client.clone());
        if mode=="offline-relay" {
            let mut connection=pool.acquire().await?;
            let mut outbox=edgeagent_outbox_postgres::PostgresOutboxRelay::new(&mut connection);
            let publisher=edgeagent_messaging_nats::JetStreamPublisher::from_context(context);
            let policy=edgeagent_outbox_relay::RelayPolicy::new("research-offline",edgeagent_messaging::LeaseDuration::new(Duration::from_secs(30))?,edgeagent_outbox_relay::MaxAttempts::new(3)?,edgeagent_outbox_relay::RetryBackoff::new(Duration::from_secs(1),Duration::from_secs(60))?)?;
            for _ in 0..maximum {
                let outcome=tokio::time::timeout(Duration::from_secs(15),edgeagent_outbox_relay::relay_once(&mut outbox,&publication::registry()?,&publisher,&policy)).await??;
                if matches!(outcome,edgeagent_outbox_relay::RelayOutcome::Idle) {break;}
            }
        } else {
            let loader=OwnerLoader::from_environment()?;
            let name=required("EDGEAGENT_RESEARCH_STREAM")?;let durable=required("EDGEAGENT_RESEARCH_CONSUMER")?;
            if !token(&name)||!token(&durable) {return Err("invalid research consumer identity".into());}
            let stream=context.get_stream(name).await?;
            let consumer:async_nats::jetstream::consumer::PullConsumer=stream.get_consumer(&durable).await?;
            let c=&consumer.cached_info().config;
            if ![DECIDED.subject()?,publication::EVALUATE.subject()?].contains(&c.filter_subject)
                || !c.filter_subjects.is_empty() || c.ack_wait<Duration::from_secs(60)
                || c.max_deliver!=3 || !(1..=32).contains(&c.max_ack_pending) {return Err("incompatible research consumer bounds".into());}
            let mut consumer=edgeagent_messaging_nats::JetStreamConsumer::new(consumer).await?;
            for _ in 0..maximum {
                let delivery=tokio::select! {_=stop()=>break,result=tokio::time::timeout(Duration::from_secs(30),consumer.receive())=>result??};
                tokio::time::timeout(Duration::from_secs(30),handle(&pool,&store,&loader,delivery,&scope)).await??;
            }
        }
        tokio::time::timeout(Duration::from_secs(5),client.drain()).await??;Ok(())
    }.await;
    tokio::time::timeout(Duration::from_secs(5), pool.close()).await?;
    result
}
/// Process and settle only after durable completion or quarantine. Unavailable reads get three attempts.
/// # Errors
/// Dependency/quarantine/settlement failures leave delivery unresolved for recovery.
pub async fn handle(
    pool: &sqlx::PgPool,
    store: &impl ResearchPublication,
    loader: &impl EvidenceLoader,
    delivery: MessageDelivery,
    scope: &str,
) -> Result {
    let envelope = MessageEnvelope::from_json(delivery.payload());
    let valid = envelope
        .as_ref()
        .ok()
        .and_then(|e| publication::evaluation_request(e).ok())
        .is_some_and(|r| r.scope == scope);
    let mut poison = !valid;
    let mut exhausted = false;
    let mut success = false;
    if valid {
        let envelope = envelope.as_ref().map_err(|_| "invalid delivery")?;
        match worker::process(store, loader, envelope, &time()?).await {
            Ok(_) => success = true,
            Err(error) if error.kind() != Kind::Storage => poison = true,
            Err(_) if delivery.metadata().delivery_attempt() >= 3 => {
                match store.recover(envelope).await? {
                    Some(_) => success = true,
                    None => {
                        poison = true;
                        exhausted = true;
                    }
                }
            }
            Err(_) => (),
        }
    }
    let disposition = if success {
        DeliveryDisposition::Acknowledge
    } else if poison {
        let evidence = QuarantineEvidence::new(
            delivery.metadata().message_key(),
            delivery.metadata().subject(),
            delivery.metadata().delivery_attempt(),
            delivery.payload(),
            if exhausted {
                "research_retry_exhausted"
            } else {
                "research_invalid"
            },
        )?;
        let mut tx = pool.begin().await?;
        PostgresInbox
            .quarantine_delivery(&mut tx, "research-worker", evidence)
            .await?;
        tx.commit().await?;
        DeliveryDisposition::Quarantined
    } else {
        DeliveryDisposition::RetryAfter(RetryDelay::new(Duration::from_secs(1))?)
    };
    delivery.settle(disposition).await?;
    Ok(())
}
async fn stop() {
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        return;
    };
    tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}}
}
