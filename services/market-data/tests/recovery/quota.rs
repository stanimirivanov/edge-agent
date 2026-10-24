//! Real, independently killed ledger actors; no provider implementation exists here.
use super::*;
use edgeagent_contracts::evidence::AcquireEvidence;
use edgeagent_market_data_domain::*;
use edgeagent_market_data_postgres::PostgresAcquisitionLedger;
use std::io::{Read, Write};
use std::num::NonZeroU32;

async fn admit(
    h: &Fixture,
    template: &AcquisitionRequest,
    id: u128,
    account: u128,
) -> Result<AcquireEvidence, TestError> {
    let mut request = *template;
    request.id = AcquisitionId::new(id)?;
    request.scope.account = ProviderAccountId::new(account)?;
    request.scope.storage = StorageScopeId::new(id)?;
    let ms = request.admitted_at.unix_millis();
    let at = |v| UtcInstant::from_unix_millis(v);
    let interval = EffectiveInterval::new(at(ms - 1000)?, at(ms + 86_400_000)?)?;
    let rights = ReferenceHistory::new(vec![ReferenceVersion::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(1)?,
        request.scope.source,
        at(ms - 1000)?,
        [7; 32],
        RightsRecord {
            scope: request.scope,
            contract: request.contract,
            agreement: ReferenceId::new(1)?,
            effective: interval,
            permissions: UsePermissions {
                automated: Permission::Allowed,
                derived: Permission::Allowed,
                canonical_storage: Permission::Allowed,
                audience: Permission::Allowed,
            },
            raw_storage: Permission::Denied,
            canonical_retention_ms: NonZeroU32::new(600_000).ok_or("retention")?,
            raw_retention_ms: None,
            delete_by: at(ms + 600_000)?,
            embargo_ms: 0,
            status: ReviewStatus::Active,
        },
    )])?;
    let providers = ReferenceHistory::new(vec![ReferenceVersion::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(1)?,
        request.scope.source,
        at(ms - 1000)?,
        [8; 32],
        ProviderReview {
            scope: request.scope,
            capability: ProviderCapability::new(
                request.scope.source,
                request.contract,
                DeliveryCapability::Snapshots,
                KnowledgeCapability::CaptureOnly,
            ),
            effective: interval,
            status: ReviewStatus::Active,
        },
    )])?;
    let authority = AuthorityCatalogs {
        rights: &rights,
        providers: &providers,
    }
    .admit(&request, request.admitted_at, &Cancellation::default())?;
    let record = AdmissionRecord::new(
        &authority,
        AcquisitionLimits {
            attempts: 3,
            spacing_ms: 15_000,
            starts_per_minute: 4,
        },
        ListingId::new(1)?,
        DailyQualityPolicy::new(TransformationRevision::new(1)?, 300_000, 1000, 100)?,
    )?;
    h.ledger().admit(&record, &authority).await?;
    h.ledger()
        .configure_daily_credits(
            request.scope.account,
            DailyCreditPolicy::new(1, 100, ms + 86_400_000, &[(request.scope.endpoint, 1)])?,
            0,
        )
        .await?;
    Ok(record.command().clone())
}

pub async fn prove(h: &mut Fixture) -> TestResult {
    let offline = h.seed(1).await?;
    let template = *h.ledger().load(&offline).await?.0.request();
    let a = admit(h, &template, 2, 1).await?;
    let b = admit(h, &template, 3, 1).await?;
    let separate = admit(h, &template, 4, 2).await?;
    let first = h.quota_child(&a)?;
    let second = h.quota_child(&b)?;
    assert_eq!(h.marker(first).await?, "ready");
    assert_eq!(h.marker(second).await?, "ready");
    h.signal(first).await?;
    h.signal(second).await?;
    let first_result = h.marker(first).await?;
    let second_result = h.marker(second).await?;
    let (winner, loser, winning_command) = match (first_result.as_str(), second_result.as_str()) {
        ("reserved", "busy") => (first, second, &a),
        ("busy", "reserved") => (second, first, &b),
        _ => {
            return Err(
                "same-account processes did not produce one winner and one fenced loser".into(),
            );
        }
    };
    assert!(h.finish(loser).await?.0.success());
    let other = h.quota_child(&separate)?;
    assert_eq!(h.marker(other).await?, "ready");
    h.signal(other).await?;
    assert_eq!(
        h.marker(other).await?,
        "reserved",
        "another account must remain independently usable"
    );
    h.kill(winner).await?;
    h.kill(other).await?;
    let charged = h.ledger().load(winning_command).await?.1;
    let other_charged = h.ledger().load(&separate).await?.1;
    let account = ProviderAccountId::new(1)?;
    let daily = h
        .ledger()
        .daily_credit_usage(account)
        .await?
        .ok_or("daily usage")?;
    assert_eq!(daily.charged, 1, "process death must retain daily credits");
    assert_eq!(
        h.ledger()
            .daily_credit_usage(ProviderAccountId::new(2)?)
            .await?
            .ok_or("other daily usage")?
            .charged,
        1
    );
    assert_eq!((charged.calls, charged.charged_bytes), (1, 1_048_576));
    assert_eq!(
        (other_charged.calls, other_charged.charged_bytes),
        (1, 1_048_576)
    );
    let permit = charged
        .pending
        .as_ref()
        .ok_or("killed process lost its uncertain permit")?;
    let other_permit = other_charged
        .pending
        .as_ref()
        .ok_or("separate account lost its permit")?;
    let losing_command = if winner == first { &b } else { &a };
    assert_eq!(
        h.ledger()
            .start_call(losing_command, None)
            .await
            .err()
            .ok_or("pending account accepted another call")?
            .kind(),
        BudgetErrorKind::Busy
    );
    h.publish(&offline, "quota-isolated-offline").await?;
    assert!(
        h.run(&["offline-worker"]).await?.contains("handled=1"),
        "synthetic work requires no external permit"
    );
    assert_eq!(h.counts().await?, (1, 1, 1));
    assert_eq!(h.ledger().load(winning_command).await?.1, charged);
    assert_eq!(
        h.ledger().daily_credit_usage(account).await?,
        Some(daily),
        "offline work must not consume daily credits"
    );
    // Only after both actual processes have been killed AND joined may the owner confirm quiescence.
    h.ledger()
        .complete_call(permit, CallCompletion::QuiescedUnknown)
        .await?;
    h.ledger()
        .complete_call(other_permit, CallCompletion::QuiescedUnknown)
        .await?;
    assert_eq!(
        h.ledger()
            .daily_credit_usage(account)
            .await?
            .ok_or("retained daily usage")?
            .charged,
        1,
        "quiescence never refunds daily credits"
    );
    h.publish(&offline, "quota-isolated-duplicate").await?;
    assert!(
        h.run(&["offline-worker"])
            .await?
            .contains("retained_receipt=1")
    );
    let state = h.ledger().load(&offline).await?.1;
    assert_eq!(
        (state.calls, state.charged_bytes),
        (0, 0),
        "synthetic work never consumes provider quota"
    );
    for command in [winning_command, &separate] {
        let retained = h.ledger().load(command).await?.1;
        assert!(retained.pending.is_none());
        assert_eq!(
            (retained.calls, retained.charged_bytes),
            (1, 1_048_576),
            "unknown completion is not a quota refund"
        );
    }
    assert_eq!(h.counts().await?, (1, 2, 1));
    assert_eq!(h.objects().await?, 1);
    assert_eq!(h.durable().await?.delivered.consumer_sequence, 2);
    Ok(())
}

fn authorized_driver(args: &[String], marker: Option<&str>) -> bool {
    marker == Some("reserve")
        && args
            .windows(2)
            .any(|pair| pair == ["--exact", "quota_child_protocol_and_process_entry"])
}
pub fn child_entry() -> TestResult {
    let args: Vec<_> = std::env::args().collect();
    let marker = std::env::var("EDGEAGENT_PR14_QUOTA_CHILD").ok();
    // Ambient credentials or a marker alone cannot turn ordinary cargo test into a database actor.
    if !authorized_driver(&args, marker.as_deref()) {
        assert!(!authorized_driver(&[], Some("reserve")));
        assert!(!authorized_driver(
            &[
                "--exact".into(),
                "quota_child_protocol_and_process_entry".into()
            ],
            Some("unknown")
        ));
        return Ok(());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime
        .block_on(actor())
        .map_err(|_| "quota fixture actor failed; private diagnostics withheld".into())
}
async fn actor() -> TestResult {
    let schema = std::env::var("EDGEAGENT_PR14_SCHEMA")?;
    if schema.len() != "edgeagent_offline_".len() + 32
        || !schema
            .strip_prefix("edgeagent_offline_")
            .is_some_and(|v| v.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("owned schema identity".into());
    }
    let json = std::env::var("EDGEAGENT_PR14_COMMAND")?;
    if json.len() > 1024 {
        return Err("command bound".into());
    }
    let command: AcquireEvidence = serde_json::from_str(&json)?;
    let url = postgres_url(&std::env::var("EDGEAGENT_POSTGRES_URL")?)?;
    let options: sqlx::postgres::PgConnectOptions = url.as_str().parse()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options.options([
            ("search_path", schema.as_str()),
            ("statement_timeout", "10000"),
            ("lock_timeout", "5000"),
        ]))
        .await?;
    let ledger = PostgresAcquisitionLedger::new(pool.clone());
    ledger.begin_attempt(&command, 1).await?;
    println!("quota=ready");
    std::io::stdout().flush()?;
    let mut signal = [0];
    std::io::stdin().read_exact(&mut signal)?;
    if signal != *b"g" {
        return Err("actor handshake".into());
    }
    match ledger.start_call(&command, None).await {
        Ok(_) => {
            println!("quota=reserved");
            std::io::stdout().flush()?;
            // The proof MUST terminate the process, never simulate death by returning or releasing.
            std::io::stdin().read_exact(&mut signal)?;
            return Err("reserved actor must be killed by its owner".into());
        }
        Err(error) if error.kind() == BudgetErrorKind::Busy => {
            println!("quota=busy");
            std::io::stdout().flush()?;
        }
        Err(error) => return Err(error.into()),
    }
    pool.close().await;
    Ok(())
}
