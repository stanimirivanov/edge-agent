//! Atomic explanation terminal records and exact companion publication intent.
use super::{PostgresResearchStore, read_snapshot, storage, transaction};
use crate::explanation_attempts::{self, Reservation};
use edgeagent_contracts::{
    explanation::{Binding, Companion, Fallback, Job, Outcome, RECORDED, Refusal},
    research::ResearchRequest,
};
use edgeagent_outbox_postgres::{EnqueueDisposition, OutboxErrorKind, PostgresOutbox};
use edgeagent_research_application::{
    explanation::ValidatedExplanation,
    explanation_context::{Grant, Permissions, Purpose},
    publication::{
        CommittedOutcome, PublicationError as Error, PublicationErrorKind as Kind, decode_decision,
    },
};
use sqlx::{Postgres, Transaction};

/// Apply after the attempt ledger and before enabling any explanation consumer.
pub const MIGRATION: &str = include_str!("../migrations/0010_explanation_completions.sql");

/// A renderable completion requires sealed validation. Refusals stay private.
pub enum Completion<'a> {
    Generated {
        explanation: &'a ValidatedExplanation,
        permit: &'a Reservation,
    },
    Fallback {
        explanation: &'a ValidatedExplanation,
        reason: Fallback,
    },
    Refused(Refusal),
}

/// Exact retained bytes for internal recovery.
pub struct Receipt {
    companion: Vec<u8>,
    publication: Option<Vec<u8>>,
}

impl Receipt {
    pub fn bytes(&self) -> &[u8] {
        &self.companion
    }
    pub fn publication(&self) -> Option<&[u8]> {
        self.publication.as_deref()
    }
}

fn invalid() -> Error {
    Error::new(Kind::Invalid)
}

pub(super) async fn exists(tx: &mut Transaction<'_, Postgres>, id: &str) -> Result<bool, Error> {
    sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_explanation_completions WHERE job_id=$1) AS "present!""#, id)
        .fetch_one(&mut **tx).await.map_err(storage)
}

async fn read(
    tx: &mut Transaction<'_, Postgres>,
    job: &Job,
    original: &CommittedOutcome,
) -> Result<Option<Receipt>, Error> {
    let id = job.id().map_err(|_| invalid())?;
    let Some(row) = sqlx::query!("SELECT companion,publication,event_time FROM edgeagent_explanation_completions WHERE job_id=$1", id)
        .fetch_optional(&mut **tx).await.map_err(storage)? else { return Ok(None); };
    let companion = Companion::decode(&row.companion).map_err(|_| invalid())?;
    let decision = decode_decision(original.decision().ok_or_else(invalid)?)?;
    companion
        .bind(original.record(), &decision)
        .map_err(|_| invalid())?;
    if companion.binding.anchor != job.anchor {
        return Err(invalid());
    }
    match (&companion.outcome, &row.publication, &row.event_time) {
        (Outcome::Refused { .. }, None, None) => {}
        (Outcome::Generated { .. } | Outcome::Fallback { .. }, Some(bytes), Some(time)) => {
            let event = companion
                .publication(original.publication(), time)
                .map_err(|_| invalid())?;
            let subject = RECORDED.subject().map_err(|_| invalid())?;
            if event.to_json().map_err(|_| invalid())? != *bytes {
                return Err(invalid());
            }
            let intact=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_message_outbox WHERE message_source=$1 AND message_id=$2 AND envelope=$3 AND message_type=$4 AND transport_subject=$5) AS "intact!""#,event.source(),event.id(),bytes,RECORDED.message_type(),subject).fetch_one(&mut **tx).await.map_err(storage)?;
            if !intact {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok(Some(Receipt {
        companion: row.companion,
        publication: row.publication,
    }))
}

fn selections(
    validated: &ValidatedExplanation,
    binding: &Binding,
) -> Result<edgeagent_contracts::explanation::Proposal, Error> {
    if validated.anchor() != &binding.anchor {
        return Err(invalid());
    }
    Ok(validated.proposal().clone())
}

impl PostgresResearchStore {
    /// Retain one explanation terminal record with its exact outbox event.
    /// The caller authenticates current evidence-owner authority and the
    /// prepared input binding. Internal exact retries recover the first bytes.
    pub async fn complete_explanation(
        &self,
        request: &ResearchRequest,
        binding: &Binding,
        completion: Completion<'_>,
        owner: Option<&Grant>,
        clock_ms: impl Fn() -> i64 + Send + Sync,
        event_time: &str,
    ) -> Result<Receipt, Error> {
        let outcome = match &completion {
            Completion::Generated { explanation, .. } => Outcome::Generated {
                proposal: selections(explanation, binding)?,
            },
            Completion::Fallback {
                explanation,
                reason,
            } => Outcome::Fallback {
                reason: *reason,
                proposal: selections(explanation, binding)?,
            },
            Completion::Refused(reason) => Outcome::Refused { reason: *reason },
        };
        let companion = Companion {
            binding: binding.clone(),
            outcome,
        };
        let bytes = companion.canonical_bytes().map_err(|_| invalid())?;
        let job = companion.job();
        if job.anchor.scope != request.scope {
            return Err(invalid());
        }
        let id = job.id().map_err(|_| invalid())?;
        let mut tx = transaction(&self.pool).await?;
        sqlx::query!(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
            request.scope
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        let original = read_snapshot(&mut tx, request)
            .await?
            .ok_or_else(invalid)?
            .receipt;
        if let Some(receipt) = read(&mut tx, &job, &original).await? {
            if receipt.companion != bytes {
                return Err(Error::new(Kind::Conflict));
            }
            tx.commit().await.map_err(storage)?;
            return Ok(receipt);
        }
        let decision = decode_decision(original.decision().ok_or_else(invalid)?)?;
        companion
            .bind(original.record(), &decision)
            .map_err(|_| invalid())?;

        let event = if matches!(completion, Completion::Refused(_)) {
            None
        } else {
            let owner = owner.ok_or_else(|| Error::new(Kind::Disabled))?;
            let serving = &owner.request;
            if serving.anchor != job.anchor
                || serving.purpose != Purpose::ExplanationServing
                || serving.document.is_some()
            {
                return Err(Error::new(Kind::Disabled));
            }
            let now = clock_ms();
            let observed = Self::current_explanation_grant(&mut tx, serving, now)
                .await?
                .ok_or_else(|| Error::new(Kind::Disabled))?;
            let grants = Permissions {
                owner: Some(owner.clone()),
                research: Some(observed.grant),
            };
            if !grants.allows(serving, now) {
                return Err(Error::new(Kind::Disabled));
            }
            Some((
                companion
                    .publication(original.publication(), event_time)
                    .map_err(|_| invalid())?,
                serving.audience.as_str(),
                observed.revision,
            ))
        };

        match completion {
            Completion::Generated { permit, .. } => {
                let reserved = &permit.intent().binding;
                if reserved.anchor != binding.anchor
                    || reserved.profile != binding.profile
                    || reserved.evidence != binding.evidence
                {
                    return Err(Error::new(Kind::Conflict));
                }
                Self::fence_explanation_result(&mut tx, permit, clock_ms).await?;
            }
            other => {
                explanation_attempts::check_terminal_binding(
                    &mut tx,
                    &id,
                    binding,
                    matches!(
                        other,
                        Completion::Fallback {
                            reason: Fallback::UncertainAttempt,
                            ..
                        }
                    ),
                )
                .await?;
            }
        }
        let publication = event
            .as_ref()
            .map(|(event, _, _)| event.to_json())
            .transpose()
            .map_err(|_| invalid())?;
        sqlx::query!("INSERT INTO edgeagent_explanation_completions(job_id,companion,publication,event_time,serving_audience,serving_revision) VALUES($1,$2,$3,$4,$5,$6)",
            id, bytes, publication.as_deref(), event.as_ref().map(|_| event_time),
            event.as_ref().map(|(_, audience, _)| *audience), event.as_ref().map(|(_, _, revision)| *revision))
            .execute(&mut *tx).await.map_err(storage)?;
        if let Some((event, _, _)) = event {
            let enqueued = PostgresOutbox
                .enqueue(&mut tx, RECORDED, &event)
                .await
                .map_err(|error| {
                    let kind = if error.kind() == OutboxErrorKind::MessageIdentityConflict {
                        Kind::Conflict
                    } else {
                        Kind::Storage
                    };
                    Error::with_source(kind, error)
                })?;
            if enqueued != EnqueueDisposition::Inserted {
                return Err(invalid());
            }
        }
        tx.commit().await.map_err(storage)?;
        Ok(Receipt {
            companion: bytes,
            publication,
        })
    }

    /// Read an intact committed companion for internal recovery without writes.
    pub async fn recover_explanation(
        &self,
        request: &ResearchRequest,
        job: &Job,
    ) -> Result<Option<Receipt>, Error> {
        if job.anchor.scope != request.scope {
            return Err(invalid());
        }
        let mut tx = transaction(&self.pool).await?;
        sqlx::query!("SET TRANSACTION READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        let result = match read_snapshot(&mut tx, request).await? {
            Some(snapshot) => read(&mut tx, job, &snapshot.receipt).await?,
            None => None,
        };
        tx.commit().await.map_err(storage)?;
        Ok(result)
    }
}
