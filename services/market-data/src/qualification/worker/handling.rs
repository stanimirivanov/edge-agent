//! Receipt-first acquisition; ownership and publication order remain visible here.
use super::*;
use edgeagent_market_data_http::{
    AlpacaDaily, AlpacaDailyConfig, AlpacaProgress, TwelveDataConfig, TwelveDataDaily,
};
use edgeagent_messaging::MessageDelivery;
use std::num::NonZeroU32;
pub(super) async fn handle(
    pool: &sqlx::PgPool,
    ledger: &Ledger,
    p: &QualificationProfile,
    delivery: MessageDelivery,
    cancel: &Cancellation,
) -> Result {
    let result = async {
        let envelope = MessageEnvelope::from_json(delivery.payload()).map_err(invalid)?;
        let command = AcquireEvidence::from_envelope(&envelope).map_err(invalid)?;
        if command != p.data().admission {
            return Err(Error::new(K::Conflict));
        }
        match ledger.recover(&envelope).await {
            Ok(Some(_)) => {
                println!("retained_receipt=1");
                return Ok(());
            }
            Ok(None) => (),
            // This concrete recovery path returns Denied for durable retirement,
            // scope denial or original retention expiry, not execution withdrawal.
            Err(e) if e.kind() == K::Denied => {
                println!("retained_denial=1");
                return Ok(());
            }
            Err(e) => return Err(e),
        }
        let (record, budget) = ledger.load(&command).await?;
        if budget.cancelled || cancel.is_cancelled() {
            return Err(Error::new(K::Cancelled));
        }
        let input = input::Input::decode(&ledger.qualification_input().await?, &record, p)?;
        let storage = storage()?;
        let attempt = u8::try_from(delivery.metadata().delivery_attempt()).map_err(invalid)?;
        let access = policy::Access {
            input: &input,
            record: &record,
            captured: now()?,
        };
        let managed = RetainedEvidenceStorage::new(&storage, ledger);
        let native_prepared = if matches!(&input.schedule, input::Schedule::Native(_)) {
            managed.recover_native(&command, &access).await?
        } else {
            None
        };
        let precise_prepared = if matches!(&input.schedule, input::Schedule::Precise(_)) {
            ledger
                .recover_precise_preparation(&command, &access)
                .await?
        } else {
            None
        };
        let recovered = native_prepared.is_some() || precise_prepared.is_some();
        if !recovered && budget.calls != 0 {
            return Err(Error::new(K::Busy));
        }
        if !recovered {
            let snapshot = ledger.qualification_snapshot().await?;
            let _report = preflight_qualification(
                p,
                &record,
                input.catalogs(),
                snapshot.at(),
                cancel,
                &snapshot,
            )?;
            external_intent(
                std::env::var_os("USE_MOCK").is_some(),
                &env("EDGEAGENT_QUALIFICATION_LIVE_OPT_IN")?,
                p,
            )?;
        }
        // Validate credentials/configuration before committing ownership. Recovery
        // uses neither a credential lookup nor a newly constructed HTTP client.
        let native_provider = if let input::Schedule::Native(schedule) = &input.schedule {
            if recovered {
                None
            } else {
                let credential = env("EDGEAGENT_TWELVE_DATA_CREDENTIAL")?;
                Some(
                    TwelveDataDaily::new(TwelveDataConfig {
                        schedule: schedule.clone(),
                        credential: &credential,
                        cancellation_poll_ms: poll()?,
                    })
                    .map_err(Error::storage)?,
                )
            }
        } else {
            None
        };
        let precise_provider = if let input::Schedule::Precise(schedule) = &input.schedule {
            if recovered {
                None
            } else {
                let key = env("EDGEAGENT_ALPACA_DATA_KEY")?;
                let secret = env("EDGEAGENT_ALPACA_DATA_SECRET")?;
                Some(
                    AlpacaDaily::new(AlpacaDailyConfig {
                        schedule: schedule.clone(),
                        key_id: &key,
                        secret: &secret,
                        cancellation_poll_ms: poll()?,
                    })
                    .map_err(Error::storage)?,
                )
            }
        } else {
            None
        };
        let claim = ledger.claim_qualification_worker().await?;
        let bound = ledger.bind_qualification_worker(&claim)?;
        let ledger = &bound;
        let managed = RetainedEvidenceStorage::new(&storage, ledger);
        if !recovered {
            ledger.begin_attempt(&command, attempt).await?;
        }
        let proposal = match &input.schedule {
            input::Schedule::Native(schedule) => {
                let old = native_prepared;
                let capture = if let Some(a) = old {
                    DateOnlyDailyArtifact::new(a.data().capture.clone()).map_err(invalid)?
                } else {
                    // A prior confirmed page without a retained preparation is not permission
                    // to spend again. Operator reconciliation/new admission is explicit.
                    if budget.calls != 0 {
                        return Err(Error::new(K::Busy));
                    }
                    let provider = native_provider.as_ref().ok_or_else(|| invalid(()))?;
                    ledger.check_qualification_worker(&claim).await?;
                    let mut session = provider
                        .start(ledger, &record, &access, cancel)
                        .await
                        .map_err(Error::storage)?;
                    session.fetch(cancel).await.map_err(Error::storage)?
                };
                ledger.check_qualification_worker(&claim).await?;
                let access = policy::Access {
                    input: &input,
                    record: &record,
                    captured: now()?,
                };
                let authorized = input
                    .catalogs()
                    .admit(record.request(), access.captured, cancel)
                    .map_err(denied)?;
                TerminalProposal::ready_native(
                    &record,
                    &authorized,
                    input.catalogs(),
                    cancel,
                    NativeEvidenceFreeze {
                        schedule,
                        capture: &capture,
                        versions: &input.versions,
                        transformation: input.transformation,
                        references: input.references(),
                        policy: input.quality,
                        previous: None,
                    },
                    &managed,
                    &access,
                )
                .await?
            }
            input::Schedule::Precise(schedule) => {
                let old = precise_prepared;
                let (batch, captured) = if let Some(a) = old {
                    let at = precise::captured(&a)?;
                    let authorized = input
                        .catalogs()
                        .admit(record.request(), at, cancel)
                        .map_err(denied)?;
                    (
                        precise::batch(&a, &record, authorized.authority().1.value().capability)?,
                        at,
                    )
                } else {
                    if budget.calls != 0 {
                        return Err(Error::new(K::Busy));
                    }
                    let provider = precise_provider.as_ref().ok_or_else(|| invalid(()))?;
                    let mut session = provider
                        .start(ledger, &record, &access, cancel)
                        .await
                        .map_err(Error::storage)?;
                    let mut result = None;
                    for page in 0..record.request().budget.pages.get() {
                        if page > 0 {
                            tokio::time::sleep(Duration::from_millis(u64::from(
                                record.limits().spacing_ms,
                            )))
                            .await;
                        }
                        if cancel.is_cancelled() {
                            return Err(Error::new(K::Cancelled));
                        }
                        ledger.check_qualification_worker(&claim).await?;
                        match session.fetch_page(cancel).await.map_err(Error::storage)? {
                            AlpacaProgress::More => (),
                            AlpacaProgress::Complete(capture) => {
                                result = Some(capture);
                                break;
                            }
                        }
                    }
                    let capture = result.ok_or_else(|| Error::new(K::Exhausted))?;
                    (capture.batch().clone(), now()?)
                };
                ledger.check_qualification_worker(&claim).await?;
                let access = policy::Access {
                    input: &input,
                    record: &record,
                    captured,
                };
                let authorized = input
                    .catalogs()
                    .admit(record.request(), access.captured, cancel)
                    .map_err(denied)?;
                let (symbol, venue, _) = schedule.mapping().value().binding();
                let buckets = schedule
                    .slots()
                    .iter()
                    .map(|s| (s.date, s.bucket))
                    .collect::<Vec<_>>();
                let series = DailySeriesSpec {
                    instrument: record.request().scope.instrument,
                    listing: record.listing()?,
                    symbol,
                    venue,
                    range: record.request().range,
                    cutoff: captured,
                    provider: batch.provider,
                    buckets: &buckets,
                };
                TerminalProposal::ready(
                    &record,
                    &authorized,
                    input.catalogs(),
                    cancel,
                    EvidenceFreeze {
                        command: &command,
                        batch: &batch,
                        series: &series,
                        references: input.references(),
                        policy: input.quality,
                        previous: None,
                    },
                    &precise::Checkpoint {
                        ledger,
                        storage: &managed,
                    },
                    &access,
                )
                .await?
            }
        };
        ledger.check_qualification_worker(&claim).await?;
        let access = policy::Access {
            input: &input,
            record: &record,
            captured: now()?,
        };
        if cancel.is_cancelled() {
            return Err(Error::new(K::Cancelled));
        }
        ledger.commit(&envelope, &proposal, &access).await?;
        // Every external operation has returned and the terminal receipt is committed.
        ledger.finish_qualification_worker(claim).await?;
        println!("terminal_committed=1");
        Ok(())
    }
    .await;
    super::settlement::settle(pool, delivery, &result, cancel.is_cancelled()).await?;
    result
}

fn poll() -> Result<NonZeroU32> {
    NonZeroU32::new(
        env("EDGEAGENT_QUALIFICATION_POLL_MS")?
            .parse()
            .map_err(invalid)?,
    )
    .filter(|v| v.get() <= 100)
    .ok_or_else(|| invalid(()))
}
