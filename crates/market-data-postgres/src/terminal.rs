//! One service-owned receipt/inbox/outbox transaction. Never performs external I/O.
use crate::{PostgresAcquisitionLedger, ledger};
use edgeagent_contracts::{MessageEnvelope, evidence::*};
use edgeagent_inbox_postgres::{DeliveryDisposition, PostgresInbox};
use edgeagent_market_data_application::{
    AcquisitionPublication, AdmissionRecord, BudgetError as Error, BudgetErrorKind as K,
    PublicationAuthority, TerminalProposal, TerminalReceipt,
};
use edgeagent_outbox_postgres::PostgresOutbox;
use sqlx::{Postgres, Transaction};
const CONSUMER: &str = "market-data-acquisition";

async fn retained(
    tx: &mut Transaction<'_, Postgres>,
    command: &AcquireEvidence,
) -> Result<Option<TerminalReceipt>, Error> {
    let row = sqlx::query!(
        "SELECT event FROM edgeagent_acquisition_receipts WHERE request_id=$1",
        command.request.as_str()
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(Error::storage)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let receipt = TerminalReceipt::decode(&row.event, command)?;
    let intact = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(SELECT 1 FROM edgeagent_message_outbox
        WHERE message_source=$1 AND message_id=$2 AND envelope=$3) AS "intact!"
    "#,
        receipt.envelope().source(),
        receipt.envelope().id(),
        receipt.bytes()
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(Error::storage)?;
    if !intact {
        return Err(Error::new(K::Invalid));
    }
    Ok(Some(receipt))
}
async fn inbox(
    tx: &mut Transaction<'_, Postgres>,
    delivery: &MessageEnvelope,
) -> Result<DeliveryDisposition, Error> {
    PostgresInbox
        .record_delivery(
            tx,
            CONSUMER,
            &registry().map_err(|_| Error::new(K::Invalid))?,
            delivery,
        )
        .await
        .map_err(|e| {
            let kind = match e.kind() {
                edgeagent_inbox_postgres::InboxErrorKind::Storage => K::Storage,
                edgeagent_inbox_postgres::InboxErrorKind::MessageIdentityConflict => K::Conflict,
                _ => K::Invalid,
            };
            Error::with_source(kind, e)
        })
}
async fn time(tx: &mut Transaction<'_, Postgres>) -> Result<(i64, String), Error> {
    let row = sqlx::query!(
        r#"
        WITH instant AS MATERIALIZED (SELECT clock_timestamp() AS at)
        SELECT floor(extract(epoch FROM at)*1000)::bigint AS "now!",
               to_char(at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS "time!"
        FROM instant
    "#
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(Error::storage)?;
    Ok((row.now, row.time))
}
fn authorize(
    record: &AdmissionRecord,
    proposal: &TerminalProposal,
    access: &impl PublicationAuthority,
) -> Result<(), Error> {
    if let EvidenceOutcome::Ready { object } = &proposal.decision().data().outcome {
        access.authorize_publication(record, object)?;
        let until = i128::from(object.delete_by.seconds) * 1000
            + i128::from(object.delete_by.nanos / 1_000_000);
        if i128::from(access.now().unix_millis()) >= until {
            return Err(Error::new(K::Deadline));
        }
    }
    Ok(())
}
impl AcquisitionPublication for PostgresAcquisitionLedger {
    async fn recover(&self, delivery: &MessageEnvelope) -> Result<Option<TerminalReceipt>, Error> {
        let command =
            AcquireEvidence::from_envelope(delivery).map_err(|_| Error::new(K::Invalid))?;
        self.bind(&command)?;
        let mut tx = self.transaction().await?;
        ledger::load(&mut tx, &command).await?; // Serialize with starts, terminal decisions and reconciliation.
        let receipt = retained(&mut tx, &command).await?;
        if receipt.is_some() {
            inbox(&mut tx, delivery).await?;
        }
        crate::retention::guard_request(&mut tx, &command.request).await?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(receipt)
    }
    async fn commit(
        &self,
        delivery: &MessageEnvelope,
        proposal: &TerminalProposal,
        access: &impl PublicationAuthority,
    ) -> Result<TerminalReceipt, Error> {
        let command =
            AcquireEvidence::from_envelope(delivery).map_err(|_| Error::new(K::Invalid))?;
        self.bind(&command)?;
        let mut tx = self.transaction().await?;
        let (record, budget) = ledger::load(&mut tx, &command).await?;
        if let Some(receipt) = retained(&mut tx, &command).await? {
            if receipt.decision() != proposal.decision() {
                return Err(Error::new(K::Conflict));
            }
            inbox(&mut tx, delivery).await?;
            crate::retention::guard_request(&mut tx, &command.request).await?;
            tx.commit().await.map_err(Error::storage)?;
            return Ok(receipt);
        }
        let (now, stamp) = time(&mut tx).await?;
        crate::qualification::guard(&mut tx, self.qualification.as_ref(), &record, None).await?;
        proposal.verify(&record, &budget.snapshot(), now)?;
        if let EvidenceOutcome::Ready { object } = &proposal.decision().data().outcome {
            crate::retention::registered(&mut tx, &record, object).await?;
        }
        authorize(&record, proposal, access)?;
        if inbox(&mut tx, delivery).await? != DeliveryDisposition::FirstDelivery {
            return Err(Error::new(K::Invalid)); // A marker without its terminal receipt is corruption.
        }
        advance(&mut tx, proposal).await?;
        let receipt = TerminalReceipt::new(delivery, proposal.decision(), &stamp)?;
        PostgresOutbox
            .enqueue(&mut tx, DEFINITIONS[1], receipt.envelope())
            .await
            .map_err(|e| {
                let kind = match e.kind() {
                    edgeagent_outbox_postgres::OutboxErrorKind::Storage => K::Storage,
                    edgeagent_outbox_postgres::OutboxErrorKind::MessageIdentityConflict => {
                        K::Conflict
                    }
                    _ => K::Invalid,
                };
                Error::with_source(kind, e)
            })?;
        sqlx::query!(
            "INSERT INTO edgeagent_acquisition_receipts (request_id,event) VALUES ($1,$2)",
            command.request.as_str(),
            receipt.bytes()
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        // Lock waits/DB work must not make ready evidence valid beyond its initial deadline.
        proposal.verify(&record, &budget.snapshot(), time(&mut tx).await?.0)?;
        crate::qualification::guard(&mut tx, self.qualification.as_ref(), &record, None).await?;
        authorize(&record, proposal, access)?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(receipt)
    }
}
async fn advance(
    tx: &mut Transaction<'_, Postgres>,
    proposal: &TerminalProposal,
) -> Result<(), Error> {
    let Some(head) = proposal.head() else {
        return Ok(());
    };
    sqlx::query!(
        "INSERT INTO edgeagent_evidence_heads (series_key) VALUES ($1) ON CONFLICT DO NOTHING",
        head.key.as_str()
    )
    .execute(&mut **tx)
    .await
    .map_err(Error::storage)?;
    let current=sqlx::query!("SELECT digest,captured_ms,cutoff_ms FROM edgeagent_evidence_heads WHERE series_key=$1 FOR UPDATE",head.key.as_str())
        .fetch_one(&mut **tx).await.map_err(Error::storage)?;
    let millis = |t: EvidenceTime| {
        i64::try_from(i128::from(t.seconds) * 1000 + i128::from(t.nanos / 1_000_000))
            .map_err(|_| Error::new(K::Invalid))
    };
    let captured = millis(head.captured_at)?;
    let cutoff = millis(head.cutoff)?;
    if current.digest.as_deref() != head.replaces.as_ref().map(EvidenceDigest::as_str)
        || current.captured_ms.is_some_and(|old| captured < old)
        || current.cutoff_ms.is_some_and(|old| cutoff < old)
    {
        return Err(Error::new(K::Conflict));
    }
    // Stable global lock order also covers overlapping ranges with shared observations.
    let mut versions = std::collections::BTreeMap::new();
    for (key, value) in proposal.observations() {
        if versions
            .insert(key.as_str(), value.as_str())
            .is_some_and(|old| old != value.as_str())
        {
            return Err(Error::new(K::Invalid));
        }
    }
    let keys = versions.keys().copied().collect::<Vec<_>>();
    let values = versions.values().copied().collect::<Vec<_>>();
    sqlx::query!("
        INSERT INTO edgeagent_observation_versions (identity,fingerprint)
        SELECT identity,fingerprint FROM unnest($1::text[],$2::text[]) AS incoming(identity,fingerprint)
        ORDER BY identity ON CONFLICT DO NOTHING",&keys as &[&str],&values as &[&str])
        .execute(&mut **tx).await.map_err(Error::storage)?;
    let conflict = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(SELECT 1 FROM unnest($1::text[],$2::text[]) AS incoming(identity,fingerprint)
        JOIN edgeagent_observation_versions AS retained USING(identity)
        WHERE incoming.fingerprint<>retained.fingerprint) AS "conflict!"
    "#,
        &keys as &[&str],
        &values as &[&str]
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(Error::storage)?;
    if conflict {
        return Err(Error::new(K::Conflict));
    }
    sqlx::query!("UPDATE edgeagent_evidence_heads SET digest=$2,captured_ms=$3,cutoff_ms=$4,owner_request=$5 WHERE series_key=$1",
        head.key.as_str(),head.digest.as_str(),captured,cutoff,proposal.decision().data().request.request.as_str()).execute(&mut **tx).await.map_err(Error::storage)?;
    sqlx::query!("INSERT INTO edgeagent_evidence_ownership(request_id,series_key,observations) VALUES($1,$2,$3)",
        proposal.decision().data().request.request.as_str(),head.key.as_str(),&keys as &[&str])
        .execute(&mut **tx).await.map_err(Error::storage)?;
    Ok(())
}
