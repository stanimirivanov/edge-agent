//! One durable send permit per explanation job.
use super::{PostgresResearchStore, storage, transaction};
use edgeagent_contracts::explanation::{Binding, Job};
use edgeagent_research_application::{
    explanation_context::{Purpose, Request},
    publication::{PublicationError as Error, PublicationErrorKind as Kind},
};
use sqlx::{Postgres, Transaction};

/// Apply after the Research disclosure grants migration.
pub const MIGRATION: &str = include_str!("../migrations/0009_explanation_attempts.sql");

/// Exact prepared model input. Deadline and digest cannot change on a retry.
#[derive(Clone)]
pub struct Intent {
    pub binding: Binding,
    pub input_digest: String,
    pub audience: String,
    pub deadline_ms: i64,
}

impl Intent {
    fn request(&self) -> Request {
        Request {
            anchor: self.binding.anchor.clone(),
            audience: self.audience.clone(),
            purpose: Purpose::ModelDisclosure,
            document: None,
        }
    }

    fn valid(&self, request: &Request) -> bool {
        self.binding.is_valid()
            && request.is_valid()
            && self.input_digest.len() == 64
            && self.input_digest.bytes().any(|b| b != b'0')
            && self
                .input_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
}

/// Only a fresh committed reservation can issue a send permit.
pub enum ReservationOutcome {
    Reserved(Reservation),
    AlreadySpent,
}

/// Send permit for one committed attempt. The caller still checks current
/// disclosure authority immediately before sending.
pub struct Reservation(Spent);

impl Reservation {
    /// Exact input authorized for this send.
    pub const fn intent(&self) -> &Intent {
        self.0.intent()
    }

    /// Research grant revision observed when the attempt was spent.
    pub const fn grant_revision(&self) -> i64 {
        self.0.grant_revision()
    }
}

/// Recovery result. No variant permits another model call.
pub enum Recovery {
    Missing,
    Terminal,
    Pending { deadline_ms: i64 },
    Fallback(Spent),
}

/// Retained input binding for the deterministic fallback path.
pub struct Spent {
    intent: Intent,
    grant_revision: i64,
}

impl Spent {
    pub const fn intent(&self) -> &Intent {
        &self.intent
    }

    pub const fn grant_revision(&self) -> i64 {
        self.grant_revision
    }
}

struct Row {
    profile: String,
    evidence: String,
    input_digest: String,
    audience: String,
    grant_revision: i64,
    reserved_at_ms: i64,
    deadline_ms: i64,
    fallback: bool,
}

fn job_id(job: &Job) -> Result<String, Error> {
    job.id().map_err(|_| Error::new(Kind::Invalid))
}

async fn locked(tx: &mut Transaction<'_, Postgres>, id: &str) -> Result<Option<Row>, Error> {
    sqlx::query_as!(Row, "SELECT profile,evidence,input_digest,audience,grant_revision,reserved_at_ms,deadline_ms,fallback FROM edgeagent_explanation_attempts WHERE job_id=$1 FOR UPDATE", id)
        .fetch_optional(&mut **tx).await.map_err(storage)
}

pub(super) async fn check_terminal_binding(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    binding: &Binding,
    requires_fallback: bool,
) -> Result<(), Error> {
    match locked(tx, id).await? {
        Some(row)
            if row.profile == binding.profile
                && row.evidence == binding.evidence
                && (!requires_fallback || row.fallback) =>
        {
            Ok(())
        }
        None if !requires_fallback => Ok(()),
        _ => Err(Error::new(Kind::Conflict)),
    }
}

impl PostgresResearchStore {
    /// Spend the one attempt before any model I/O. The caller authenticates
    /// the job and the evidence-owner permissions separately.
    /// The clock is sampled after acquiring the scope fence.
    pub async fn reserve_explanation_attempt(
        &self,
        intent: &Intent,
        clock_ms: impl FnOnce() -> i64 + Send,
    ) -> Result<ReservationOutcome, Error> {
        let request = intent.request();
        if !intent.valid(&request) {
            return Err(Error::new(Kind::Invalid));
        }
        let job = Job {
            anchor: intent.binding.anchor.clone(),
        };
        let id = job_id(&job)?;
        let mut tx = transaction(&self.pool).await?;
        sqlx::query!(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
            job.anchor.scope
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        if let Some(row) = locked(&mut tx, &id).await? {
            if row.profile != intent.binding.profile
                || row.evidence != intent.binding.evidence
                || row.input_digest != intent.input_digest
                || row.audience != intent.audience
                || row.deadline_ms != intent.deadline_ms
            {
                return Err(Error::new(Kind::Conflict));
            }
            tx.commit().await.map_err(storage)?;
            return Ok(ReservationOutcome::AlreadySpent);
        }
        if super::explanation_completion::exists(&mut tx, &id).await? {
            tx.commit().await.map_err(storage)?;
            return Ok(ReservationOutcome::AlreadySpent);
        }
        let now_ms = clock_ms();
        if now_ms < 0
            || !intent
                .deadline_ms
                .checked_sub(now_ms)
                .is_some_and(|ms| (1..=30_000).contains(&ms))
        {
            return Err(Error::new(Kind::Invalid));
        }
        let grant = Self::current_explanation_grant(&mut tx, &request, now_ms)
            .await?
            .filter(|grant| intent.deadline_ms <= grant.grant.expires_at_ms)
            .ok_or_else(|| Error::new(Kind::Disabled))?;
        sqlx::query!("INSERT INTO edgeagent_explanation_attempts(job_id,profile,evidence,input_digest,audience,grant_revision,reserved_at_ms,deadline_ms) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
            id,intent.binding.profile,intent.binding.evidence,intent.input_digest,intent.audience,grant.revision,now_ms,intent.deadline_ms)
            .execute(&mut *tx).await.map_err(storage)?;
        tx.commit().await.map_err(storage)?;
        Ok(ReservationOutcome::Reserved(Reservation(Spent {
            intent: intent.clone(),
            grant_revision: grant.revision,
        })))
    }

    /// After the deadline, record deterministic fallback for the spent attempt.
    /// Repeated recovery returns the retained inputs without changing them.
    pub async fn recover_explanation_attempt(
        &self,
        job: &Job,
        clock_ms: impl FnOnce() -> i64 + Send,
    ) -> Result<Recovery, Error> {
        let id = job_id(job)?;
        let mut tx = transaction(&self.pool).await?;
        sqlx::query!(
            "SELECT pg_advisory_xact_lock_shared(hashtextextended($1,0))",
            job.anchor.scope
        )
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        if super::explanation_completion::exists(&mut tx, &id).await? {
            tx.commit().await.map_err(storage)?;
            return Ok(Recovery::Terminal);
        }
        let result = match locked(&mut tx, &id).await? {
            None => Recovery::Missing,
            Some(row) => {
                let now_ms = clock_ms();
                if now_ms < 0 {
                    return Err(Error::new(Kind::Invalid));
                }
                if !row.fallback && now_ms < row.deadline_ms {
                    Recovery::Pending {
                        deadline_ms: row.deadline_ms,
                    }
                } else {
                    if !row.fallback {
                        sqlx::query!("UPDATE edgeagent_explanation_attempts SET fallback=TRUE WHERE job_id=$1", id)
                            .execute(&mut *tx).await.map_err(storage)?;
                    }
                    Recovery::Fallback(Spent {
                        intent: Intent {
                            binding: Binding {
                                anchor: job.anchor.clone(),
                                profile: row.profile,
                                evidence: row.evidence,
                            },
                            input_digest: row.input_digest,
                            audience: row.audience,
                            deadline_ms: row.deadline_ms,
                        },
                        grant_revision: row.grant_revision,
                    })
                }
            }
        };
        tx.commit().await.map_err(storage)?;
        Ok(result)
    }

    /// Fence a model result in the transaction that will retain it. The row
    /// stays locked until transaction end. Use READ COMMITTED and acquire any
    /// required scope authority before calling this method.
    pub async fn fence_explanation_result(
        tx: &mut Transaction<'_, Postgres>,
        permit: &Reservation,
        clock_ms: impl FnOnce() -> i64 + Send,
    ) -> Result<(), Error> {
        let id = job_id(&Job {
            anchor: permit.intent().binding.anchor.clone(),
        })?;
        let row = locked(tx, &id)
            .await?
            .ok_or_else(|| Error::new(Kind::Invalid))?;
        let now_ms = clock_ms();
        if row.fallback
            || super::explanation_completion::exists(tx, &id).await?
            || row.grant_revision != permit.grant_revision()
            || !(row.reserved_at_ms..row.deadline_ms).contains(&now_ms)
        {
            return Err(Error::new(Kind::Conflict));
        }
        Ok(())
    }
}
