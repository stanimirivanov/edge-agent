//! Schedule exact Research publications without contacting an evidence owner or model.
use super::{
    DeliveryDisposition, Error, Kind, MessageEnvelope, PostgresResearchStore, ResearchRecord,
    publication, read_snapshot, record_delivery, storage, transaction,
};
use edgeagent_contracts::explanation::{Anchor, Job};

/// Apply after Research decisions and explanation completions in the private schema.
pub const MIGRATION: &str = include_str!("../migrations/0011_explanation_jobs.sql");
/// Stable inbox and quarantine identity for artifact scheduling.
pub const CONSUMER: &str = "research-explanation-jobs";

fn invalid() -> Error {
    Error::new(Kind::Invalid)
}

impl PostgresResearchStore {
    /// Accept one exact owner artifact publication in the same transaction as its job.
    /// A verified legacy artifact without a decision is acknowledged without a job.
    ///
    /// # Errors
    /// Rejects foreign scope, unsupported or changed publications, and storage failures.
    pub async fn admit_explanation_job(
        &self,
        scope: &str,
        event: &MessageEnvelope,
    ) -> Result<(), Error> {
        publication::PUBLISHED
            .validate_envelope(event)
            .map_err(|_| invalid())?;
        let record: ResearchRecord = event.payload().map_err(|_| invalid())?;
        let ResearchRecord::Artifact(artifact) = &record else {
            return Err(invalid());
        };
        if artifact.request.scope != scope {
            return Err(invalid());
        }

        let mut tx = transaction(&self.pool).await?;
        let receipt = read_snapshot(&mut tx, &artifact.request)
            .await?
            .ok_or_else(invalid)?
            .receipt;
        if receipt.publication().to_json().map_err(|_| invalid())?
            != event.to_json().map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        let delivery = record_delivery(&mut tx, CONSUMER, event).await?;
        if let Some(decision) = receipt.decision() {
            let decision = publication::decode_decision(decision)?;
            let job = Job {
                anchor: Anchor {
                    scope: scope.to_owned(),
                    artifact: receipt.record().digest().map_err(|_| invalid())?,
                    decision: decision.digest().map_err(|_| invalid())?,
                },
            };
            job.bind(receipt.record(), &decision)
                .map_err(|_| invalid())?;
            let id = job.id().map_err(|_| invalid())?;
            let bytes = job.canonical_bytes().map_err(|_| invalid())?;
            let request = &artifact.request.request_id;
            match delivery {
                DeliveryDisposition::FirstDelivery => {
                    sqlx::query!(
                        "INSERT INTO edgeagent_research_explanation_jobs (job_id,caller_scope,request_id,job) VALUES ($1,$2,$3,$4)",
                        id, scope, request, bytes,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(storage)?;
                }
                DeliveryDisposition::Duplicate => {
                    let intact = sqlx::query_scalar!(
                        r#"SELECT EXISTS(SELECT 1 FROM edgeagent_research_explanation_jobs WHERE job_id=$1 AND caller_scope=$2 AND request_id=$3 AND job=$4) AS "intact!""#,
                        id, scope, request, bytes,
                    )
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(storage)?;
                    if !intact {
                        return Err(Error::new(Kind::Storage));
                    }
                }
            }
        }
        tx.commit().await.map_err(storage)
    }
}
