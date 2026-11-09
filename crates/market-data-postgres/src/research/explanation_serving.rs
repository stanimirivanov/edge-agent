//! Current owner review and retention checks for derived explanation serving.
use super::{PostgresResearchEvidence, invalid};
use crate::retention;
use edgeagent_contracts::{
    MessageEnvelope,
    evidence::{EvidenceId, EvidenceOutcome},
    research_acquisition::ResearchEvidence,
};
use edgeagent_market_data_application::{
    AdmissionRecord, BudgetError as Error, BudgetErrorKind as K, EvidenceAccess,
};
use serde_json::Value;
use sqlx::{Postgres, Transaction};

/// Bounded operator command. The scope is derived from a sealed admission.
pub struct ExplanationServingReview {
    audience: EvidenceId,
    agreement: EvidenceId,
    revision: i64,
    decision: String,
    valid_from_ms: i64,
    expires_at_ms: i64,
}

impl ExplanationServingReview {
    /// Decode the local operator command and reject unsupported fields.
    ///
    /// # Errors
    /// Rejects malformed, oversized or invalid commands.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > 1024 {
            return Err(invalid());
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        let fields = value
            .as_object()
            .filter(|map| map.len() == 6)
            .ok_or_else(invalid)?;
        let text = |name: &str| fields.get(name).and_then(Value::as_str).ok_or_else(invalid);
        let integer = |name: &str| fields.get(name).and_then(Value::as_i64).ok_or_else(invalid);
        let review = Self {
            audience: EvidenceId::new(text("audience")?).map_err(|_| invalid())?,
            agreement: EvidenceId::new(text("agreement")?).map_err(|_| invalid())?,
            revision: integer("revision")?,
            decision: text("decision")?.into(),
            valid_from_ms: integer("valid_from_ms")?,
            expires_at_ms: integer("expires_at_ms")?,
        };
        if review.revision <= 0
            || review.valid_from_ms < 0
            || review.expires_at_ms <= review.valid_from_ms
            || !matches!(review.decision.as_str(), "allow" | "deny" | "revoke")
        {
            return Err(invalid());
        }
        Ok(review)
    }
}

struct Latest {
    revision: i64,
    decision: String,
    agreement: String,
    valid_from_ms: i64,
    expires_at_ms: i64,
    actor: String,
}

async fn latest(
    tx: &mut Transaction<'_, Postgres>,
    scope: &str,
    audience: &EvidenceId,
) -> Result<Option<Latest>, Error> {
    sqlx::query_as!(Latest, r#"SELECT revision,decision,agreement,valid_from_ms,expires_at_ms,actor::text AS "actor!" FROM edgeagent_explanation_serving_reviews WHERE rights_scope=$1 AND audience=$2 ORDER BY revision DESC LIMIT 1"#,
        scope, audience.as_str()
    ).fetch_optional(&mut **tx).await.map_err(Error::storage)
}

impl PostgresResearchEvidence {
    /// Append the next review as the authenticated schema owner.
    ///
    /// # Errors
    /// Rejects unknown admissions, stale revisions, foreign actors and storage failure.
    pub async fn review_explanation_serving(
        &self,
        acquisition: &EvidenceId,
        review: &ExplanationServingReview,
    ) -> Result<(), Error> {
        let scope = self
            .owner_record(acquisition)
            .await?
            .explanation_rights_scope()?;
        let mut tx = self.tx().await?;
        let actor=sqlx::query_scalar!(r#"SELECT session_user::text AS "actor!" FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner WHERE n.nspname=current_schema() AND r.rolname=session_user"#)
            .fetch_optional(&mut *tx).await.map_err(Error::storage)?.ok_or_else(|| Error::new(K::Denied))?;
        let lock = format!("explanation-serving/{scope}/{}", review.audience.as_str());
        sqlx::query!("SELECT pg_advisory_xact_lock(hashtextextended($1,0))", lock)
            .execute(&mut *tx)
            .await
            .map_err(Error::storage)?;
        if let Some(last) = latest(&mut tx, &scope, &review.audience).await? {
            if last.revision == review.revision
                && last.decision == review.decision
                && last.agreement == review.agreement.as_str()
                && last.valid_from_ms == review.valid_from_ms
                && last.expires_at_ms == review.expires_at_ms
                && last.actor == actor
            {
                return tx.commit().await.map_err(Error::storage);
            }
            if last.revision.checked_add(1) != Some(review.revision) {
                return Err(Error::new(K::Conflict));
            }
        } else if review.revision != 1 {
            return Err(Error::new(K::Conflict));
        }
        sqlx::query!("INSERT INTO edgeagent_explanation_serving_reviews(rights_scope,audience,revision,decision,agreement,valid_from_ms,expires_at_ms,actor) VALUES($1,$2,$3,$4,$5,$6,$7,session_user)",
            scope, review.audience.as_str(), review.revision, review.decision,
            review.agreement.as_str(), review.valid_from_ms, review.expires_at_ms)
            .execute(&mut *tx).await.map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)
    }

    /// Reconcile exact owner evidence, then check current retention and serving review.
    /// The caller provides the owner-local current-rights adapter after verifying storage.
    ///
    /// # Errors
    /// Denies foreign, absent, retired or unauthorized evidence and fails closed on storage errors.
    pub async fn authorize_explanation_serving(
        &self,
        event: &MessageEnvelope,
        admission: &AdmissionRecord,
        audience: &EvidenceId,
        access: &impl EvidenceAccess,
    ) -> Result<(), Error> {
        let denied = || Error::new(K::Denied);
        let ResearchEvidence::Ready { request, decision } =
            ResearchEvidence::from_envelope(event).map_err(|_| invalid())?
        else {
            return Err(denied());
        };
        let EvidenceOutcome::Ready { object } = &decision.outcome else {
            return Err(invalid());
        };
        if &decision.request != admission.command() {
            return Err(denied());
        }
        let scope = admission.explanation_rights_scope()?;
        let mut tx = self.tx().await?;
        // Hold the retention scope before the Research correlation lock.
        retention::active(&mut tx, admission).await?;
        retention::guard_request(&mut tx, &decision.request.request).await?;
        Self::fence(&mut tx, &request).await?;
        let retained = Self::read(&mut tx, &request).await?;
        if retained.admission != *admission
            || retained
                .publication
                .ok_or_else(denied)?
                .to_json()
                .map_err(|_| invalid())?
                != event.to_json().map_err(|_| invalid())?
        {
            return Err(denied());
        }
        retention::registered(&mut tx, admission, object).await?;
        access
            .authorize(object)
            .map_err(|cause| Error::with_source(K::Denied, cause))?;
        let allowed = sqlx::query_scalar!(r#"
            SELECT decision='allow' AND valid_from_ms<=clock.now_ms AND clock.now_ms<expires_at_ms
                AND clock.now_ms<$3 AS "allowed!"
            FROM edgeagent_explanation_serving_reviews
            CROSS JOIN (SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS now_ms) clock
            WHERE rights_scope=$1 AND audience=$2 ORDER BY revision DESC LIMIT 1
        "#,scope, audience.as_str(), object.delete_by.seconds.checked_mul(1000)
            .and_then(|ms| ms.checked_add(i64::from(object.delete_by.nanos / 1_000_000))).ok_or_else(invalid)?)
            .fetch_optional(&mut *tx).await.map_err(Error::storage)?;
        if allowed != Some(true) {
            return Err(denied());
        }
        tx.commit().await.map_err(Error::storage)
    }
}
