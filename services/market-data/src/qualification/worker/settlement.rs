//! A durable quarantine/receipt precedes terminal broker settlement.
use super::*;
use edgeagent_messaging::{DeliveryDisposition, MessageDelivery, RetryDelay};
pub(super) async fn settle(
    pool: &sqlx::PgPool,
    delivery: MessageDelivery,
    result: &Result,
    cancelled: bool,
) -> Result {
    let permanent = result
        .as_ref()
        .err()
        .is_some_and(|e| matches!(e.kind(), K::Invalid | K::Conflict));
    let disposition = if permanent && !cancelled {
        let evidence = edgeagent_inbox_postgres::QuarantineEvidence::new(
            delivery.metadata().message_key(),
            delivery.metadata().subject(),
            delivery.metadata().delivery_attempt(),
            delivery.payload(),
            "qualification_rejected",
        )
        .map_err(invalid)?;
        let mut tx = pool.begin().await.map_err(Error::storage)?;
        edgeagent_inbox_postgres::PostgresInbox
            .quarantine_delivery(&mut tx, "market-data-qualification", evidence)
            .await
            .map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)?;
        DeliveryDisposition::Quarantined
    } else if result.is_ok() {
        DeliveryDisposition::Acknowledge
    } else {
        DeliveryDisposition::RetryAfter(RetryDelay::new(Duration::from_secs(15)).map_err(invalid)?)
    };
    delivery.settle(disposition).await.map_err(Error::storage)?;
    Ok(())
}
