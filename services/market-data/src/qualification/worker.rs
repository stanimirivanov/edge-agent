//! Receipt-first finite intake. No reconnect/refetch loop, no automatic fence release on error.
mod handling;
mod settlement;
use super::*;
use edgeagent_messaging::MessageConsumer;
use edgeagent_messaging_nats::JetStreamConsumer;
use handling::handle;

pub(super) async fn run(pool: &sqlx::PgPool, ledger: &Ledger, p: &QualificationProfile) -> Result {
    let client = tokio::time::timeout(
        Duration::from_secs(5),
        async_nats::connect(broker_url(env("EDGEAGENT_QUALIFICATION_NATS_URL")?)?),
    )
    .await
    .map_err(Error::storage)?
    .map_err(Error::storage)?;
    let result=async{
        let js=async_nats::jetstream::new(client.clone());
        let stream=js.get_stream(env("EDGEAGENT_QUALIFICATION_STREAM")?).await.map_err(Error::storage)?;
        let consumer:async_nats::jetstream::consumer::PullConsumer=stream.get_consumer(&env("EDGEAGENT_QUALIFICATION_CONSUMER")?).await.map_err(|e|Error::storage(Dependency(e)))?;
        let cfg=&consumer.cached_info().config;
        if cfg.filter_subject!=DEFINITIONS[0].subject().map_err(invalid)?||!cfg.filter_subjects.is_empty()||cfg.ack_wait<Duration::from_secs(330)||cfg.max_deliver<1||cfg.max_deliver>3||cfg.max_ack_pending!=1{return Err(invalid(()));}
        let mut consumer=JetStreamConsumer::new(consumer).await.map_err(Error::storage)?;
        let stop=stop_signal();tokio::pin!(stop);
        let cancel=Cancellation::default();
        let delivery=tokio::select!{
            signal=&mut stop=>{signal?;return Err(Error::new(K::Cancelled));},
            result=tokio::time::timeout(Duration::from_secs(30),consumer.receive())=>result.map_err(Error::storage)?.map_err(Error::storage)?,
        };
        let work=tokio::time::timeout(Duration::from_secs(300),handle(pool,ledger,p,delivery,&cancel));
        tokio::pin!(work);
        tokio::select!{
            r=&mut work=>r.map_err(Error::storage)?,
            signal=&mut stop=>{signal?;cancel.cancel();tokio::time::timeout(Duration::from_secs(15),&mut work).await.map_err(Error::storage)?.map_err(Error::storage)?}
        }
    }.await;
    let drain = tokio::time::timeout(Duration::from_secs(5), client.drain())
        .await
        .map_err(Error::storage)?
        .map_err(Error::storage);
    result.and(drain)
}
async fn stop_signal() -> Result {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .map_err(Error::storage)?;
        tokio::select! {r=tokio::signal::ctrl_c()=>r.map_err(Error::storage)?,_=terminate.recv()=>()}
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.map_err(Error::storage)?;
    Ok(())
}

struct Dependency(Box<dyn std::error::Error + Send + Sync>);
impl std::fmt::Debug for Dependency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dependency(..)")
    }
}
impl std::fmt::Display for Dependency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("qualification dependency failed")
    }
}
impl std::error::Error for Dependency {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}
