//! Receipt-first handling and confirmed settlement; runtime lifecycle is separate.
use super::*;
pub(super) async fn handle(
    pool: &sqlx::PgPool,
    ledger: &PostgresAcquisitionLedger,
    storage: &ImmutableEvidenceStore,
    delivery: MessageDelivery,
    cancel: &Cancellation,
    confirmed: &mut BTreeSet<EvidenceId>,
) -> Result {
    let attempt = u8::try_from(delivery.metadata().delivery_attempt())?;
    let result: Result = async {
        let envelope = MessageEnvelope::from_json(delivery.payload())
            .map_err(|_| Permanent("inbound_contract_invalid"))?;
        let command = AcquireEvidence::from_envelope(&envelope)
            .map_err(|_| Permanent("inbound_contract_invalid"))?;
        match ledger.recover(&envelope).await {
            Ok(Some(_)) => {
                // Completed work is independent of today's compiled source recipe.
                // Recovery is historical receipt delivery, never an object-read grant.
                println!("retained_receipt=1");
                return Ok(());
            }
            Ok(None) => (),
            // This concrete adapter returns Denied only after reading persisted retirement,
            // scope withdrawal or immutable retention bounds. Never regenerate a receipt.
            Err(error) if error.kind() == BudgetErrorKind::Denied => {
                println!("retained_denial=1");
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }
        let (record, _) = ledger.load(&command).await?;
        let job = Job::restore(&record).map_err(|_| Permanent("offline_configuration_mismatch"))?;
        let access = Access {
            job: &job,
            captured: now()?,
        };
        let managed = RetainedEvidenceStorage::new(storage, ledger);
        let wrote = AtomicBool::new(false);
        finish_acquisition(ledger, &envelope, &access, || async {
            ledger.begin_attempt(&command, attempt).await?;
            let proposal =
                job.prepare(&managed, &access, cancel)
                    .await
                    .map_err(|cause| match cause.downcast::<BudgetError>() {
                        Ok(error) => *error,
                        Err(cause) => BudgetError::with_source(
                            BudgetErrorKind::Invalid,
                            PreparationError(cause),
                        ),
                    })?;
            wrote.store(true, Ordering::SeqCst);
            Ok(proposal)
        })
        .await?;
        if wrote.load(Ordering::SeqCst) {
            confirmed.insert(command.request);
        }
        Ok(())
    }
    .await;
    let permanent = result.as_ref().err().and_then(|error| {
        error.downcast_ref::<Permanent>().map(|e| e.0).or_else(|| {
            error.downcast_ref::<BudgetError>().and_then(|e| {
                matches!(
                    e.kind(),
                    BudgetErrorKind::Invalid
                        | BudgetErrorKind::Conflict
                        | BudgetErrorKind::Exhausted
                )
                .then_some("acquisition_rejected")
            })
        })
    });
    let disposition = if let Some(code) = permanent.filter(|_| !cancel.is_cancelled()) {
        let evidence = edgeagent_inbox_postgres::QuarantineEvidence::new(
            delivery.metadata().message_key(),
            delivery.metadata().subject(),
            delivery.metadata().delivery_attempt(),
            delivery.payload(),
            code,
        )?;
        let mut tx = pool.begin().await?;
        edgeagent_inbox_postgres::PostgresInbox
            .quarantine_delivery(&mut tx, "market-data-offline", evidence)
            .await?;
        tx.commit().await?;
        DeliveryDisposition::Quarantined
    } else if result.is_ok() {
        DeliveryDisposition::Acknowledge
    } else {
        DeliveryDisposition::RetryAfter(RetryDelay::new(Duration::from_secs(15))?)
    };
    delivery.settle(disposition).await?;
    result
}

#[derive(Debug)]
struct Permanent(&'static str);
impl std::fmt::Display for Permanent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Permanent {}

struct PreparationError(Box<dyn std::error::Error + Send + Sync>);
impl std::fmt::Debug for PreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreparationError(..)")
    }
}
impl std::fmt::Display for PreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("offline preparation failed")
    }
}
impl std::error::Error for PreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}
