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

/// Authenticated durable job and its intact original owner completion.
pub struct Scheduled {
    pub job: Job,
    pub request: edgeagent_contracts::research::ResearchRequest,
    pub original: super::ReplaySnapshot,
    pub receipt: Option<super::explanation_completion::Receipt>,
}
impl PostgresResearchStore {
    /// Read an explicit job for recovery, or the oldest unfinished scoped job.
    /// # Errors
    /// Rejects changed retained identity and storage failures.
    pub async fn explanation_job(
        &self,
        scope: &str,
        id: Option<&str>,
    ) -> Result<Option<Scheduled>, Error> {
        let mut tx = transaction(&self.pool).await?;
        let row=sqlx::query!("SELECT j.job_id,j.job,c.request FROM edgeagent_research_explanation_jobs j JOIN edgeagent_research_completions c USING(caller_scope,request_id) WHERE j.caller_scope=$1 AND ($2::text IS NULL OR j.job_id=$2) AND ($2::text IS NOT NULL OR NOT EXISTS(SELECT 1 FROM edgeagent_explanation_completions e WHERE e.job_id=j.job_id)) ORDER BY j.admitted_at,j.job_id LIMIT 1",scope,id)
            .fetch_optional(&mut *tx).await.map_err(storage)?;
        let Some(row) = row else { return Ok(None) };
        let job = Job::decode(&row.job).map_err(|_| invalid())?;
        if job.id().map_err(|_| invalid())? != row.job_id || job.anchor.scope != scope {
            return Err(invalid());
        }
        let ResearchRecord::Request(request) =
            ResearchRecord::decode(&row.request).map_err(|_| invalid())?
        else {
            return Err(invalid());
        };
        let original = read_snapshot(&mut tx, &request)
            .await?
            .ok_or_else(invalid)?;
        let decision =
            publication::decode_decision(original.receipt.decision().ok_or_else(invalid)?)?;
        job.bind(original.receipt.record(), &decision)
            .map_err(|_| invalid())?;
        let receipt = super::explanation_completion::read(&mut tx, &job, &original.receipt).await?;
        tx.commit().await.map_err(storage)?;
        Ok(Some(Scheduled {
            job,
            request,
            original,
            receipt,
        }))
    }

    /// Resolve the original owner event only for a fresh job's evidence verification.
    /// # Errors
    /// Refuses absent, changed or foreign original input.
    pub async fn explanation_input(&self, scheduled: &Scheduled) -> Result<MessageEnvelope, Error> {
        let publication = scheduled.original.receipt.publication();
        let cause = publication.extension("causationid").ok_or_else(invalid)?;
        let source = edgeagent_contracts::Component::MarketData.source_uri();
        let kind = edgeagent_contracts::research_acquisition::DECIDED.message_type();
        let bytes=sqlx::query_scalar!("SELECT envelope FROM edgeagent_message_inbox WHERE consumer_name='research-completion' AND message_source=$1 AND message_id=$2 AND message_type=$3",source,cause,kind)
            .fetch_optional(&self.pool).await.map_err(storage)?.ok_or_else(invalid)?;
        let event = MessageEnvelope::from_json(&bytes).map_err(|_| invalid())?;
        if publication::evaluation_request(&event)? != scheduled.request
            || event.extension("correlationid") != publication.extension("correlationid")
        {
            return Err(invalid());
        }
        Ok(event)
    }
}
