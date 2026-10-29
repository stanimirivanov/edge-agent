//! Explicit local serving and finite projection intake; provisioning is an operator task.
use crate::{
    error::Kind,
    http,
    store::{ProjectionStore, token},
};
use edgeagent_contracts::{MessageEnvelope, research::ResearchRecord};
use edgeagent_messaging::{DeliveryDisposition, MessageConsumer, RetryDelay};
use edgeagent_research_application::publication::{ABSTAINED, PUBLISHED};
use std::{net::SocketAddr, sync::Arc, time::Duration};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn required(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "missing projector setting".into())
}
/// Validate local infrastructure without routing override escape hatches.
pub fn local(value: &str, schemes: &[&str]) -> Result {
    let url = url::Url::parse(value)?;
    if !schemes.contains(&url.scheme())
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.host_str().is_some_and(|v| {
            v == "localhost"
                || v.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        })
    {
        return Err("projector requires loopback infrastructure".into());
    }
    Ok(())
}
/// Refuse nonlocal listener addresses before starting HTTP.
pub fn listen(value: &str) -> Result<SocketAddr> {
    let address: SocketAddr = value.parse()?;
    if !address.ip().is_loopback() {
        return Err("projector listener must be loopback".into());
    }
    Ok(address)
}
/// Explicit modes leave descriptor commands and schema/topology untouched.
pub fn run(args: &[String]) -> Result {
    if args.len() != 1
        || !matches!(
            args[0].as_str(),
            "serve" | "offline-track" | "offline-once" | "offline-worker"
        )
    {
        return Err("invalid projector mode".into());
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
    let scope = required("EDGEAGENT_PROJECTOR_SCOPE")?;
    let schema = required("EDGEAGENT_PROJECTOR_SCHEMA")?;
    if !schema.starts_with("projector_")
        || schema.len() > 63
        || !schema
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err("invalid projector schema".into());
    }
    let address = if mode == "serve" {
        Some(listen(&required("EDGEAGENT_PROJECTOR_LISTEN")?)?)
    } else {
        None
    };
    let options: sqlx::postgres::PgConnectOptions = database.parse()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options.options([
            ("search_path", schema.as_str()),
            ("statement_timeout", "15000"),
            ("lock_timeout", "5000"),
        ]))
        .await?;
    let store = Arc::new(ProjectionStore::new(pool.clone(), scope)?);
    let result:Result=async {
        sqlx::query!("SELECT request_id FROM edgeagent_research_views LIMIT 0").fetch_all(&pool).await?;
        if let Some(address)=address {
            let listener=tokio::net::TcpListener::bind(address).await?;
            axum::serve(listener,http::router(store.clone())).with_graceful_shutdown(stop()).await?;return Ok(());
        }
        if mode!="offline-worker" {
            use std::io::Read as _;let mut bytes=Vec::new();std::io::stdin().take(262_145).read_to_end(&mut bytes)?;
            if mode=="offline-track" {
                let ResearchRecord::Request(request)=ResearchRecord::decode(&bytes)? else {return Err("expected trusted request".into());};
                store.track(request).await?;
            } else {
                let envelope=MessageEnvelope::from_json(&bytes)?;
                store.project(&envelope,&format!("manual/{}",envelope.deduplication_key())).await?;
            }return Ok(());
        }
        let broker=required("EDGEAGENT_NATS_URL")?;local(&broker,&["nats","tls"])?;
        let name=required("EDGEAGENT_PROJECTOR_STREAM")?;let durable=required("EDGEAGENT_PROJECTOR_CONSUMER")?;
        let maximum=required("EDGEAGENT_PROJECTOR_MAX_MESSAGES")?.parse::<u8>()?;
        if !token(&name)||!token(&durable)||!(1..=16).contains(&maximum) {return Err("invalid projector work bounds".into());}
        let client=tokio::time::timeout(Duration::from_secs(5),async_nats::connect(broker)).await??;
        let context=async_nats::jetstream::new(client.clone());let stream=context.get_stream(name).await?;
        let consumer:async_nats::jetstream::consumer::PullConsumer=stream.get_consumer(&durable).await?;
        let c=&consumer.cached_info().config;
        let mut expected=vec![PUBLISHED.subject()?,ABSTAINED.subject()?];expected.sort();let mut actual=c.filter_subjects.clone();actual.sort();
        if !c.filter_subject.is_empty() || actual!=expected || c.ack_wait<Duration::from_secs(60) || c.max_deliver!=3 || !(1..=32).contains(&c.max_ack_pending) {return Err("incompatible projector consumer bounds".into());}
        let mut consumer=edgeagent_messaging_nats::JetStreamConsumer::new(consumer).await?;
        for _ in 0..maximum {
            let delivery=tokio::select! {_=stop()=>break,result=tokio::time::timeout(Duration::from_secs(30),consumer.receive())=>result??};
            let envelope=MessageEnvelope::from_json(delivery.payload());
            let valid=envelope.as_ref().is_ok_and(|e| {
                let def=if e.message_type()==PUBLISHED.message_type() {PUBLISHED} else {ABSTAINED};
                def.subject().is_ok_and(|s|s==delivery.metadata().subject())
            });
            let result=if valid {Some(tokio::time::timeout(Duration::from_secs(25),store.project(envelope.as_ref().map_err(|_|"invalid envelope")?,delivery.metadata().message_key())).await?)} else {None};
            let disposition=match result {
                Some(Ok(_))=>DeliveryDisposition::Acknowledge,
                Some(Err(e)) if matches!(e.kind(),Kind::Gap|Kind::Storage|Kind::Corrupt)=>DeliveryDisposition::RetryAfter(RetryDelay::new(Duration::from_secs(15))?),
                _=>{
                    let evidence=edgeagent_inbox_postgres::QuarantineEvidence::new(delivery.metadata().message_key(),delivery.metadata().subject(),delivery.metadata().delivery_attempt(),delivery.payload(),"projector_invalid")?;
                    let mut tx=pool.begin().await?;edgeagent_inbox_postgres::PostgresInbox.quarantine_delivery(&mut tx,"research-projector",evidence).await?;tx.commit().await?;DeliveryDisposition::Quarantined
                },
            };
            delivery.settle(disposition).await?;
        }
        tokio::time::timeout(Duration::from_secs(5),client.drain()).await??;Ok(())
    }.await;
    tokio::time::timeout(Duration::from_secs(5), pool.close()).await?;
    result
}
async fn stop() {
    let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        return;
    };
    tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
}
