//! Transport settlement for Research's published-artifact scheduler.
use edgeagent_contracts::MessageEnvelope;
use edgeagent_inbox_postgres::{PostgresInbox, QuarantineEvidence};
use edgeagent_messaging::{DeliveryDisposition, MessageDelivery, RetryDelay};
use edgeagent_research_application::publication::{PUBLISHED, PublicationErrorKind};
use edgeagent_research_postgres::{PostgresResearchStore, explanation_jobs::CONSUMER};
use std::time::Duration;

pub(crate) async fn handle(
    pool: &sqlx::PgPool,
    store: &PostgresResearchStore,
    delivery: MessageDelivery,
    scope: &str,
) -> super::runtime::Result {
    let metadata = delivery.metadata();
    let subject = PUBLISHED.subject()?;
    if metadata.subject() == subject
        && let Ok(event) = MessageEnvelope::from_json(delivery.payload())
    {
        match store.admit_explanation_job(scope, &event).await {
            Ok(()) => {
                delivery.settle(DeliveryDisposition::Acknowledge).await?;
                return Ok(());
            }
            Err(error) if error.kind() == PublicationErrorKind::Storage => {
                if metadata.delivery_attempt() >= 3 {
                    return Err(error.into());
                }
                delivery
                    .settle(DeliveryDisposition::RetryAfter(RetryDelay::new(
                        Duration::from_secs(1),
                    )?))
                    .await?;
                return Ok(());
            }
            Err(_) => {}
        }
    }
    let evidence = QuarantineEvidence::new(
        metadata.message_key(),
        metadata.subject(),
        metadata.delivery_attempt(),
        delivery.payload(),
        "research_explanation_invalid",
    )?;
    let mut tx = pool.begin().await?;
    PostgresInbox
        .quarantine_delivery(&mut tx, CONSUMER, evidence)
        .await?;
    tx.commit().await?;
    delivery.settle(DeliveryDisposition::Quarantined).await?;
    Ok(())
}
