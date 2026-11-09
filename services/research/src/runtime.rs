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
use edgeagent_telemetry::{
    EventSpineContext, EventSpineOutcome, EventSpineStage, record_event_spine_operation,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
pub(crate) type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
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
pub(crate) fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
pub(crate) fn clock_ms() -> Result<i64> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}
fn time() -> Result<String> {
    Ok(chrono::DateTime::from_timestamp_millis(clock_ms()?)
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
            "offline-experiment"
                | "offline-governed-once"
                | "offline-governance-install"
                | "offline-governance-approve"
                | "offline-governance-retire"
                | "offline-control"
                | "offline-explanation-grant"
                | "offline-explanation-authority"
                | "offline-experiment-ledger"
                | "offline-once"
                | "offline-worker"
                | "offline-explanation-scheduler"
                | "offline-relay"
                | "offline-lifecycle"
                | "offline-invalidation-check"
                | "offline-replay"
                | "offline-explain"
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
    if mode == "offline-experiment" {
        use std::io::{Read as _, Write as _};
        let mut bytes = Vec::new();
        std::io::stdin().take(262_145).read_to_end(&mut bytes)?;
        let delivery = MessageEnvelope::from_json(&bytes)?;
        let request = publication::evaluation_request(&delivery)?;
        // This branch owns no Research pool, store, approval or broker connection.
        let loader = OwnerLoader::from_environment()?;
        loader.validate_request(&request)?;
        if !matches!(
            edgeagent_contracts::research_acquisition::ResearchEvidence::from_envelope(&delivery)?,
            edgeagent_contracts::research_acquisition::ResearchEvidence::Ready { .. }
        ) {
            return Err("experiment requires owner-ready evidence".into());
        }
        let result = tokio::time::timeout(Duration::from_secs(25), async {
            let loaded = loader.load(&delivery).await?;
            edgeagent_research_application::experiment::evaluate(
                request,
                &loaded.policy,
                &loaded.series,
                loaded.expires_at_ms,
            )
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        })
        .await??;
        std::io::stdout().write_all(result.bytes())?;
        return Ok(());
    }
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
    let authority = if mode == "offline-explanation-authority" {
        let address: std::net::SocketAddr =
            required("EDGEAGENT_RESEARCH_EXPLANATION_LISTEN")?.parse()?;
        if !address.ip().is_loopback() {
            return Err("Research explanation authority requires a loopback listener".into());
        }
        let audience = edgeagent_contracts::evidence::EvidenceId::new(&required(
            "EDGEAGENT_RESEARCH_EXPLANATION_AUDIENCE",
        )?)?;
        Some((
            address,
            audience,
            crate::loader::OwnerEndpoint::from_environment()?,
        ))
    } else {
        None
    };
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
        if let Some((address, audience, owner)) = authority {
            let listener = tokio::net::TcpListener::bind(address).await?;
            let router = crate::explanation_authority::owner_router(scope, audience, store, owner);
            axum::serve(listener, router).with_graceful_shutdown(stop()).await?;
            return Ok(());
        }
        if mode == "offline-experiment-ledger" {
            use std::io::{Read as _, Write as _};
            use edgeagent_contracts::research_experiment::{Command, MAX_COMMAND_BYTES};
            let mut bytes = Vec::new();
            std::io::stdin()
                .take((MAX_COMMAND_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            let command = Command::decode(&bytes)?;
            let Command::V1 { scope: requested, .. } = &command;
            if requested != &scope {
                return Err("foreign experiment scope".into());
            }
            let retained = tokio::time::timeout(
                Duration::from_secs(25),
                store.experiment_ledger(&command),
            ).await??;
            std::io::stdout().write_all(&retained)?;
            return Ok(());
        }
        if mode == "offline-explanation-grant" {
            use std::io::Read as _;
            use edgeagent_research_application::explanation_context::{Grant, Purpose, Request};
            #[derive(serde::Deserialize)]
            #[serde(tag = "kind", deny_unknown_fields)]
            enum Command {
                #[serde(rename = "research.explanation.grant.v1")]
                V1 {
                    anchor: edgeagent_contracts::explanation::Anchor,
                    audience: String,
                    purpose: String,
                    document: Option<String>,
                    revision: i64,
                    valid_from_ms: i64,
                    expires_at_ms: i64,
                    revoked: bool,
                },
            }
            let mut bytes = Vec::new();
            std::io::stdin().take(4097).read_to_end(&mut bytes)?;
            if bytes.len() > 4096 { return Err("oversized grant command".into()); }
            let Command::V1 { anchor, audience, purpose, document, revision, valid_from_ms, expires_at_ms, revoked } =
                serde_json::from_slice(&bytes).map_err(|_| "invalid grant command")?;
            if anchor.scope != scope { return Err("foreign grant scope".into()); }
            let purpose = match purpose.as_str() {
                "research_use" => Purpose::ResearchUse,
                "model_disclosure" => Purpose::ModelDisclosure,
                "explanation_serving" => Purpose::ExplanationServing,
                _ => return Err("invalid grant purpose".into()),
            };
            let grant = Grant { request: Request { anchor, audience, purpose, document }, valid_from_ms, expires_at_ms, revoked };
            store.set_explanation_grant(&grant, revision).await?;
            return Ok(());
        }
        if mode=="offline-control" {
            use std::io::Read as _;
            let mut bytes=Vec::new();std::io::stdin().take((edgeagent_contracts::research_controls::MAX_CONTROL_BYTES+1) as u64).read_to_end(&mut bytes)?;
            let command=edgeagent_contracts::research_controls::Command::decode(&bytes)?;
            let edgeagent_contracts::research_controls::Command::V1{scope:requested,..}=&command;
            if requested!=&scope {return Err("foreign control scope".into());}
            store.control(&command).await?;return Ok(());
        }
        if mode.starts_with("offline-governance-") {
            use std::io::Read as _;
            let mut bytes=Vec::new(); std::io::stdin().take(4097).read_to_end(&mut bytes)?;
            if mode == "offline-governance-approve"
                && let Ok(evidence) = edgeagent_contracts::research_promotion::Promotion::decode(&bytes)
            {
                if evidence.profile.scope != scope { return Err("foreign governance scope".into()); }
                store.promote(&evidence).await?;
                return Ok(());
            }
            let profile=edgeagent_contracts::strategy_governance::Profile::decode(&bytes)?;
            if profile.scope!=scope {return Err("foreign governance scope".into());}
            let revision=match mode {"offline-governance-install"=>1,"offline-governance-approve"=>2,_=>3};
            store.govern(&profile,revision).await?;return Ok(());
        }
        if matches!(mode, "offline-replay" | "offline-explain") {
            use std::io::{Read as _,Write as _};let mut bytes=Vec::new();std::io::stdin().take(262_145).read_to_end(&mut bytes)?;
            if bytes.len() > 262_144 { return Err("oversized research input".into()); }
            let delivery=MessageEnvelope::from_json(&bytes)?;
            if publication::evaluation_request(&delivery)?.scope!=scope {return Err("foreign replay scope".into());}
            let renderer=required("EDGEAGENT_RESEARCH_REPLAY_RENDERER")?;let momentum=required("EDGEAGENT_RESEARCH_REPLAY_MOMENTUM")?;
            let result=tokio::time::timeout(Duration::from_secs(25),async {
                use edgeagent_research_application::replay;
                let request=publication::evaluation_request(&delivery)?;
                let snapshot=store.retained_for_replay(&delivery).await?.ok_or("replay completion not retained")?;
                let bytes=if let Some(selection)=snapshot.selection {
                    let loader=OwnerLoader::from_retained_profile(&selection.profile(&request.scope))?;
                    let verified=replay::verify_execution(&loader,&delivery,&snapshot.receipt,selection,&renderer,&momentum).await?;
                    if mode == "offline-explain" {
                        use edgeagent_research_application::explanation::Allowlist;
                        let envelope = snapshot.receipt.decision().ok_or("explanation requires retained decision")?;
                        let decision = publication::decode_decision(envelope)?;
                        Allowlist::from_verified(snapshot.receipt.record(), &decision)?.render()?.into_bytes()
                    } else {
                        verified
                    }
                } else {
                    if mode == "offline-explain" {
                        return Err("explanation requires retained decision".into());
                    }
                    let loader=OwnerLoader::from_environment()?;
                    loader.validate_request(&request)?;
                    replay::verify(&loader,&delivery,&snapshot.receipt,&renderer,&momentum).await?.bytes().to_vec()
                };
                Result::<Vec<u8>>::Ok(bytes)
            }).await??;
            std::io::stdout().write_all(&result)?;return Ok(());
        }
        if mode=="offline-invalidation-check" {
            use std::io::{Read as _,Write as _};
            use edgeagent_contracts::research_invalidation::{Command,MAX_COMMAND_BYTES};
            let mut bytes=Vec::new();std::io::stdin().take((MAX_COMMAND_BYTES+1) as u64).read_to_end(&mut bytes)?;
            let command=Command::decode(&bytes)?;
            if let Some(event)=tokio::time::timeout(Duration::from_secs(25),store.check_invalidation(&scope,&command,&time()?)).await?? {
                std::io::stdout().write_all(&event.to_json()?)?;
            }
            return Ok(());
        }
        if mode=="offline-lifecycle" {
            use std::io::{Read as _,Write as _};let mut bytes=Vec::new();std::io::stdin().take(131_073).read_to_end(&mut bytes)?;
            let edgeagent_contracts::research::ResearchRecord::Lifecycle(event)=edgeagent_contracts::research::ResearchRecord::decode(&bytes)? else {return Err("expected lifecycle owner command".into());};
            let event=tokio::time::timeout(Duration::from_secs(25),store.append_lifecycle(&scope,event,&time()?)).await??;
            std::io::stdout().write_all(&event.to_json()?)?;return Ok(());
        }
        if mode=="offline-once" || mode=="offline-governed-once" {
            use std::io::{Read as _,Write as _};let mut bytes=Vec::new();std::io::stdin().take(262_145).read_to_end(&mut bytes)?;
            let delivery=MessageEnvelope::from_json(&bytes)?;
            if publication::evaluation_request(&delivery)?.scope!=scope {return Err("foreign research scope".into());}
            if let Some(receipt)=store.recover_existing(&delivery).await? {std::io::stdout().write_all(receipt.bytes())?;return Ok(());}
            let loader=OwnerLoader::from_environment()?;
            let selection=execution_selection()?;
            let profile=selection.profile(&scope);
            if loader.governance_profile(profile.lookback_sessions)!=profile {return Err("execution profile mismatch".into());}
            let store=PostgresResearchStore::governed(pool.clone(),profile);
            let receipt=tokio::time::timeout(Duration::from_secs(25),worker::process_execution(&store,&loader,selection,&delivery,&time()?)).await??;
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
            let name=required("EDGEAGENT_RESEARCH_STREAM")?;let durable=required("EDGEAGENT_RESEARCH_CONSUMER")?;
            if !token(&name)||!token(&durable) {return Err("invalid research consumer identity".into());}
            let stream=context.get_stream(name).await?;
            let consumer:async_nats::jetstream::consumer::PullConsumer=stream.get_consumer(&durable).await?;
            let c=&consumer.cached_info().config;
            let scheduler = mode == "offline-explanation-scheduler";
            let valid_filter = if scheduler {
                c.filter_subject == publication::PUBLISHED.subject()?
            } else {
                [DECIDED.subject()?, publication::EVALUATE.subject()?].contains(&c.filter_subject)
            };
            if !valid_filter
                || !c.filter_subjects.is_empty() || c.ack_wait<Duration::from_secs(60)
                || c.max_deliver!=3 || !(1..=32).contains(&c.max_ack_pending) {return Err("incompatible research consumer bounds".into());}
            let mut consumer=edgeagent_messaging_nats::JetStreamConsumer::new(consumer).await?;
            let evaluation = if scheduler {
                None
            } else {
                let loader=OwnerLoader::from_environment()?;
                let selected=execution_selection()?.profile(&scope);
                if loader.governance_profile(selected.lookback_sessions)!=selected {return Err("execution profile mismatch".into());}
                Some((PostgresResearchStore::governed(pool.clone(),selected),loader))
            };
            for _ in 0..maximum {
                let delivery=tokio::select! {_=stop()=>break,result=tokio::time::timeout(Duration::from_secs(30),consumer.receive())=>result??};
                tokio::time::timeout(Duration::from_secs(30),async {
                    if let Some((store,loader))=&evaluation {
                        handle(&pool,store,loader,delivery,&scope).await
                    } else {
                        crate::explanation_jobs::handle(&pool,&store,delivery,&scope).await
                    }
                }).await??;
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
    loader: &impl worker::ExecutionLoader,
    delivery: MessageDelivery,
    scope: &str,
) -> Result {
    let handling = Instant::now();
    let envelope = MessageEnvelope::from_json(delivery.payload());
    let valid = envelope
        .as_ref()
        .ok()
        .and_then(|e| publication::evaluation_request(e).ok())
        .is_some_and(|r| r.scope == scope);
    let mut poison = !valid;
    let mut exhausted = false;
    let mut success = false;
    let mut disabled = false;
    if valid {
        let envelope = envelope.as_ref().map_err(|_| "invalid delivery")?;
        match worker::process_execution(store, loader, execution_selection()?, envelope, &time()?)
            .await
        {
            Ok(_) => success = true,
            Err(error) if error.kind() == Kind::Disabled => disabled = true,
            Err(error) if error.kind() != Kind::Storage => poison = true,
            Err(_) if delivery.metadata().delivery_attempt() >= 3 => {
                match store.recover(envelope).await {
                    Ok(Some(_)) => success = true,
                    Err(error) if error.kind() == Kind::Disabled => disabled = true,
                    Err(error) => return Err(error.into()),
                    Ok(None) => {
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
        DeliveryDisposition::RetryAfter(RetryDelay::new(Duration::from_secs(if disabled {
            15
        } else {
            1
        }))?)
    };
    let outcome = match disposition {
        DeliveryDisposition::Acknowledge => EventSpineOutcome::Succeeded,
        DeliveryDisposition::Quarantined => EventSpineOutcome::Quarantined,
        DeliveryDisposition::RetryAfter(_) => EventSpineOutcome::RetryScheduled,
    };
    let context = match &envelope {
        Ok(envelope) => EventSpineContext::from_envelope(envelope),
        Err(_) => EventSpineContext::from_delivery(delivery.metadata()),
    };
    let attempt = delivery.metadata().delivery_attempt();
    record_event_spine_operation(
        &context,
        EventSpineStage::Handling,
        outcome,
        attempt,
        handling.elapsed(),
    );
    let settling = Instant::now();
    let settled = delivery.settle(disposition).await;
    record_event_spine_operation(
        &context,
        EventSpineStage::Acknowledgement,
        if settled.is_ok() {
            outcome
        } else {
            EventSpineOutcome::Failed
        },
        attempt,
        settling.elapsed(),
    );
    settled?;
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

fn execution_selection() -> Result<edgeagent_research_application::execution::Selection> {
    use std::io::Read as _;
    let path = required("EDGEAGENT_RESEARCH_EXECUTION_CONFIGURATION")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((edgeagent_contracts::strategy::MAX_BUNDLE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(edgeagent_research_application::execution::Selection::decode(&bytes)?)
}
