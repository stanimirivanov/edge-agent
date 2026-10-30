//! Explicit owner-selected offline evidence handoff; no client-supplied execution inputs.
use super::*;
use edgeagent_contracts::{
    Component, MessageMetadata,
    research::AbstentionReason as Reason,
    research_acquisition::{self, ResearchEvidence},
};
use edgeagent_market_data_postgres::PostgresResearchEvidence;

pub(super) async fn fulfil(
    pool: sqlx::PgPool,
    ledger: &PostgresAcquisitionLedger,
    storage: &ImmutableEvidenceStore,
    delivery: &MessageEnvelope,
    owner_id: u128,
    scope: &str,
    cancel: &Cancellation,
) -> Result<MessageEnvelope> {
    let request = research_acquisition::request(delivery)?;
    if request.scope != scope {
        return Err(BudgetError::new(BudgetErrorKind::Denied).into());
    }
    let bridge = PostgresResearchEvidence::new(pool);
    if bridge.denied(delivery).await? {
        return Err(BudgetError::new(BudgetErrorKind::Denied).into());
    }
    let id = EvidenceId::new(&format!("{owner_id:032x}"))?;
    let record = bridge.owner_record(&id).await?;
    let pending = bridge.reserve(delivery, &record).await?;
    if let Some(event) = pending.publication {
        return Ok(event);
    }
    // Restore only a compiled owner recipe, using the retained first binding on restart.
    let job = Job::restore(&pending.admission)?;
    // Restore admits only Single (one session) or the bounded six-session owner recipes.
    // Compare the immutable profile, without changing its code-byte configuration fingerprint.
    let single = Job::with_recipe(
        job.record.request().id.get(),
        job.record.request().admitted_at.unix_millis(),
        fixture::Recipe::Single,
    )?;
    let sessions = if single.record == job.record { 1 } else { 6 };
    let outcome = match edgeagent_market_data_application::research::select(
        &request,
        &job.record,
        sessions,
    ) {
        Err(reason) => ResearchEvidence::Abstained {
            request: request.clone(),
            reason,
        },
        Ok(()) => {
            let (key, _) = research_acquisition::binding(&request)?;
            let command = job.record.command();
            let partition = format!("acquisition/{}", command.request.as_str());
            let acquisition = MessageEnvelope::from_payload(
                MessageMetadata {
                    id: format!("research-acquire-{key}"),
                    source: Component::MarketData.source_uri().into(),
                    message_type: DEFINITIONS[0].message_type().into(),
                    subject: partition.clone(),
                    time: serde_json::from_slice::<serde_json::Value>(
                        &pending.delivery.to_json()?,
                    )?["time"]
                        .as_str()
                        .ok_or("missing command time")?
                        .into(),
                    data_schema: DEFINITIONS[0].data_schema().into(),
                    correlation_id: key,
                    causation_id: pending.delivery.id().into(),
                    idempotency_key: command.request_sha256.as_str().into(),
                    partition_key: partition,
                    trace_parent: pending
                        .delivery
                        .extension("traceparent")
                        .ok_or("missing trace context")?
                        .into(),
                    trace_state: None,
                },
                command,
            )?;
            let access = Access {
                job: &job,
                captured: now()?,
            };
            let managed = RetainedEvidenceStorage::new(storage, ledger);
            let result: Result<TerminalReceipt> = async {
                if let Some(receipt) = ledger.recover(&acquisition).await? {
                    return Ok(receipt);
                }
                ledger.begin_attempt(command, 1).await?;
                let proposal = job.prepare(&managed, &access, cancel).await?;
                Ok(ledger.commit(&acquisition, &proposal, &access).await?)
            }
            .await;
            match result {
                Ok(receipt) => match &receipt.decision().data().outcome {
                    EvidenceOutcome::Ready { object } => {
                        match access.authorize_publication(&job.record, object) {
                            Ok(()) => ResearchEvidence::Ready {
                                request: request.clone(),
                                decision: Box::new(receipt.decision().data().clone()),
                            },
                            Err(error) => ResearchEvidence::Abstained {
                                request: request.clone(),
                                reason: classification(&error)?,
                            },
                        }
                    }
                    EvidenceOutcome::Rejected { reason } => ResearchEvidence::Abstained {
                        request: request.clone(),
                        reason: match reason {
                            EvidenceRejection::Rights => Reason::MissingEntitlement,
                            _ => Reason::InvalidEvidence,
                        },
                    },
                },
                Err(error) => {
                    let reason = classification(error.as_ref())?;
                    let rejection = match reason {
                        Reason::MissingEntitlement => EvidenceRejection::Rights,
                        Reason::StaleEvidence => EvidenceRejection::Deadline,
                        _ => EvidenceRejection::Quality,
                    };
                    // Keep the existing acquisition terminal when owner retention permits it.
                    // Revocation itself may forbid that write; correlation still records denial.
                    if let Err(error) = ledger
                        .commit(
                            &acquisition,
                            &TerminalProposal::rejected(command.clone(), rejection),
                            &access,
                        )
                        .await
                        && error.kind() != BudgetErrorKind::Denied
                    {
                        return Err(error.into());
                    }
                    ResearchEvidence::Abstained {
                        request: request.clone(),
                        reason,
                    }
                }
            }
        }
    };
    if cancel.is_cancelled() {
        return Err(BudgetError::new(BudgetErrorKind::Cancelled).into());
    }
    Ok(bridge.complete(delivery, &outcome).await?)
}
fn classification(error: &(dyn std::error::Error + 'static)) -> Result<Reason> {
    let mut cause = Some(error);
    while let Some(error) = cause {
        if let Some(e) = error.downcast_ref::<BudgetError>() {
            match e.kind() {
                BudgetErrorKind::Storage
                | BudgetErrorKind::Conflict
                | BudgetErrorKind::Cancelled => return Err(BudgetError::new(e.kind()).into()),
                BudgetErrorKind::Denied => return Ok(Reason::MissingEntitlement),
                BudgetErrorKind::Deadline => return Ok(Reason::StaleEvidence),
                _ => (),
            }
        }
        if error
            .downcast_ref::<EvidenceStorageError>()
            .is_some_and(|e| e.kind() == EvidenceStorageErrorKind::Backend)
        {
            return Err(BudgetError::new(BudgetErrorKind::Storage).into());
        }
        if let Some(e) = error.downcast_ref::<AcquisitionError>() {
            return Ok(match e {
                AcquisitionError::Rights => Reason::MissingEntitlement,
                AcquisitionError::Deadline => Reason::StaleEvidence,
                AcquisitionError::Cancelled => {
                    return Err(BudgetError::new(BudgetErrorKind::Cancelled).into());
                }
                _ => Reason::InvalidEvidence,
            });
        }
        cause = error.source();
    }
    Ok(Reason::InvalidEvidence)
}
pub(super) async fn handle(
    pool: &sqlx::PgPool,
    ledger: &PostgresAcquisitionLedger,
    storage: &ImmutableEvidenceStore,
    delivery: MessageDelivery,
    owner_id: u128,
    scope: &str,
    cancel: &Cancellation,
) -> Result {
    let envelope = MessageEnvelope::from_json(delivery.payload());
    let invalid = envelope
        .as_ref()
        .ok()
        .and_then(|e| research_acquisition::request(e).ok())
        .is_none_or(|r| r.scope != scope);
    let result = if invalid {
        Err(BudgetError::new(BudgetErrorKind::Invalid).into())
    } else {
        fulfil(
            pool.clone(),
            ledger,
            storage,
            &envelope?,
            owner_id,
            scope,
            cancel,
        )
        .await
        .map(|_| ())
    };
    if !invalid
        && result
            .as_ref()
            .err()
            .and_then(|e| e.downcast_ref::<BudgetError>())
            .is_some_and(|e| e.kind() == BudgetErrorKind::Denied)
        && PostgresResearchEvidence::new(pool.clone())
            .denied(&MessageEnvelope::from_json(delivery.payload())?)
            .await?
    {
        delivery.settle(DeliveryDisposition::Acknowledge).await?;
        return Ok(());
    }
    let permanent = invalid
        || result
            .as_ref()
            .err()
            .and_then(|e| e.downcast_ref::<BudgetError>())
            .is_some_and(|e| {
                matches!(
                    e.kind(),
                    BudgetErrorKind::Invalid | BudgetErrorKind::Conflict
                )
            });
    let disposition = if permanent && !cancel.is_cancelled() {
        let evidence = edgeagent_inbox_postgres::QuarantineEvidence::new(
            delivery.metadata().message_key(),
            delivery.metadata().subject(),
            delivery.metadata().delivery_attempt(),
            delivery.payload(),
            "research_evidence_rejected",
        )?;
        let mut tx = pool.begin().await?;
        edgeagent_inbox_postgres::PostgresInbox
            .quarantine_delivery(&mut tx, "market-data-research", evidence)
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
