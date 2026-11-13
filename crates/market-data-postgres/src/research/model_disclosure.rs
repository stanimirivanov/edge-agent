//! Separate current owner authority for disclosure to an immutable model recipient.
use super::{PendingResearchEvidence, PostgresResearchEvidence, invalid};
use crate::retention;
use edgeagent_contracts::{
    canonical_json_bytes,
    evidence::{DailyEvidenceArtifact, EvidenceId, EvidenceOutcome},
    model_disclosure::{Permit, Request},
    research_acquisition::ResearchEvidence,
};
use edgeagent_market_data_application::{
    BudgetError as Error, BudgetErrorKind as K, EvidenceAccess,
};
use serde_json::Value;
use sqlx::{Postgres, Transaction};

/// Canonical bounded schema-owner review command; never a caller grant.
pub struct ModelDisclosureReview {
    request: Request,
    revision: i64,
    decision: String,
    agreement: EvidenceId,
    valid_from_ms: i64,
    expires_at_ms: i64,
}
impl ModelDisclosureReview {
    /// Decode strict canonical JSON plus LF with exactly six command fields.
    /// # Errors
    /// Rejects oversized, duplicate, malformed or unsupported command data.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > 10240 {
            return Err(invalid());
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        let mut canonical = canonical_json_bytes(&value).map_err(|_| invalid())?;
        canonical.push(b'\n');
        if canonical != bytes {
            return Err(invalid());
        }
        let map = value
            .as_object()
            .filter(|map| map.len() == 6)
            .ok_or_else(invalid)?;
        let text = |name: &str| map.get(name).and_then(Value::as_str).ok_or_else(invalid);
        let integer = |name: &str| map.get(name).and_then(Value::as_i64).ok_or_else(invalid);
        let review = Self {
            request: serde_json::from_value(map.get("request").ok_or_else(invalid)?.clone())
                .map_err(|_| invalid())?,
            revision: integer("revision")?,
            decision: text("decision")?.into(),
            agreement: EvidenceId::new(text("agreement")?).map_err(|_| invalid())?,
            valid_from_ms: integer("valid_from_ms")?,
            expires_at_ms: integer("expires_at_ms")?,
        };
        review.request.canonical_bytes().map_err(|_| invalid())?;
        if review.revision <= 0
            || review.valid_from_ms < 0
            || review.expires_at_ms <= review.valid_from_ms
            || !matches!(review.decision.as_str(), "allow" | "deny" | "revoke")
        {
            return Err(invalid());
        }
        Ok(review)
    }
    /// Exact reviewed recipient and input contract; acquisition remains owner-resolved.
    pub fn request(&self) -> &Request {
        &self.request
    }
}
struct Latest {
    revision: i64,
    request: Vec<u8>,
    rights_scope: String,
    decision: String,
    agreement: String,
    valid_from_ms: i64,
    expires_at_ms: i64,
    actor: String,
}
async fn lock(tx: &mut Transaction<'_, Postgres>, key: &str) -> Result<(), Error> {
    let key = format!("model-disclosure/{key}");
    sqlx::query!("SELECT pg_advisory_xact_lock(hashtextextended($1,0))", key)
        .execute(&mut **tx)
        .await
        .map_err(Error::storage)?;
    Ok(())
}
async fn latest(tx: &mut Transaction<'_, Postgres>, key: &str) -> Result<Option<Latest>, Error> {
    sqlx::query_as!(Latest,r#"SELECT revision,request,rights_scope,decision,agreement,valid_from_ms,expires_at_ms,actor::text AS "actor!" FROM edgeagent_model_disclosure_reviews WHERE review_key=$1 ORDER BY revision DESC LIMIT 1"#,key).fetch_optional(&mut **tx).await.map_err(Error::storage)
}
impl PostgresResearchEvidence {
    /// Append one audited review with schema-owner identity and consecutive revision.
    /// # Errors
    /// Refuses foreign actors, missing admissions and conflicting retries/revisions.
    pub async fn review_model_disclosure(
        &self,
        review: &ModelDisclosureReview,
    ) -> Result<(), Error> {
        let scope = self
            .owner_record(&review.request.acquisition)
            .await?
            .model_disclosure_rights_scope()?;
        let key = review.request.review_key(&scope).map_err(|_| invalid())?;
        let request = review.request.canonical_bytes().map_err(|_| invalid())?;
        let mut tx = self.tx().await?;
        let actor=sqlx::query_scalar!(r#"SELECT session_user::text AS "actor!" FROM pg_namespace n JOIN pg_roles r ON r.oid=n.nspowner WHERE n.nspname=current_schema() AND r.rolname=session_user"#).fetch_optional(&mut *tx).await.map_err(Error::storage)?.ok_or_else(||Error::new(K::Denied))?;
        lock(&mut tx, &key).await?;
        if let Some(last) = latest(&mut tx, &key).await? {
            if last.revision == review.revision
                && last.request == request
                && last.rights_scope == scope
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
        sqlx::query!("INSERT INTO edgeagent_model_disclosure_reviews(review_key,rights_scope,revision,request,decision,agreement,valid_from_ms,expires_at_ms,actor) VALUES($1,$2,$3,$4,$5,$6,$7,$8,session_user)",key,scope,review.revision,request,review.decision,review.agreement.as_str(),review.valid_from_ms,review.expires_at_ms).execute(&mut *tx).await.map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)
    }
    async fn model_subject(
        tx: &mut Transaction<'_, Postgres>,
        request: &Request,
    ) -> Result<PendingResearchEvidence, Error> {
        let denied = || Error::new(K::Denied);
        request.canonical_bytes().map_err(|_| invalid())?;
        Self::fence(tx, &request.research).await?;
        let retained = Self::read(tx, &request.research).await?;
        let event = retained.publication.as_ref().ok_or_else(denied)?;
        let ResearchEvidence::Ready { decision, .. } =
            ResearchEvidence::from_envelope(event).map_err(|_| invalid())?
        else {
            return Err(denied());
        };
        let EvidenceOutcome::Ready { object } = &decision.outcome else {
            return Err(denied());
        };
        if decision.request != *retained.admission.command()
            || decision.request.request != request.acquisition
            || object.sha256 != request.evidence
            || DailyEvidenceArtifact::hash(&event.to_json().map_err(|_| invalid())?).as_str()
                != request.publication
        {
            return Err(denied());
        }
        Ok(retained)
    }
    /// Resolve exact historical owner metadata for verified storage access. No grant.
    /// # Errors
    /// Refuses pending, foreign or damaged owner publications.
    pub async fn model_disclosure_evidence(
        &self,
        request: &Request,
    ) -> Result<PendingResearchEvidence, Error> {
        let mut tx = self.tx().await?;
        let subject = Self::model_subject(&mut tx, request).await?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(subject)
    }
    /// Fresh current owner review/rights/retention observation, not a bearer lease.
    /// Caller supplies owner-local rights after verifying storage; Research must
    /// separately attest the actual runtime, prepared bytes and artifact anchor.
    /// # Errors
    /// Denies absent, superseded, revoked, expired or foreign authority/evidence.
    pub async fn model_disclosure_permit(
        &self,
        request: &Request,
        access: &impl EvidenceAccess,
    ) -> Result<Permit, Error> {
        let denied = || Error::new(K::Denied);
        request.canonical_bytes().map_err(|_| invalid())?;
        let admission = self.owner_record(&request.acquisition).await?;
        let scope = admission.model_disclosure_rights_scope()?;
        let key = request.review_key(&scope).map_err(|_| invalid())?;
        let mut tx = self.tx().await?;
        retention::active(&mut tx, &admission).await?;
        retention::guard_request(&mut tx, &request.acquisition).await?;
        let retained = Self::model_subject(&mut tx, request).await?;
        if retained.admission != admission {
            return Err(denied());
        }
        let ResearchEvidence::Ready { decision, .. } =
            ResearchEvidence::from_envelope(retained.publication.as_ref().ok_or_else(denied)?)
                .map_err(|_| invalid())?
        else {
            return Err(denied());
        };
        let EvidenceOutcome::Ready { object } = &decision.outcome else {
            return Err(denied());
        };
        retention::registered(&mut tx, &admission, object).await?;
        lock(&mut tx, &key).await?;
        access
            .authorize(object)
            .map_err(|cause| Error::with_source(K::Denied, cause))?;
        let review = latest(&mut tx, &key).await?.ok_or_else(denied)?;
        let checked_at = sqlx::query_scalar!(
            r#"SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!""#
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        if review.rights_scope != scope
            || Request::decode(&review.request)
                .map_err(|_| invalid())?
                .review_key(&scope)
                .map_err(|_| invalid())?
                != key
            || review.decision != "allow"
            || checked_at < review.valid_from_ms
            || checked_at >= review.expires_at_ms
        {
            return Err(denied());
        }
        let retention_until = object
            .delete_by
            .seconds
            .checked_mul(1000)
            .and_then(|ms| ms.checked_add(i64::from(object.delete_by.nanos / 1_000_000)))
            .ok_or_else(invalid)?
            .min(admission.delete_by_ms()?);
        let permit = Permit {
            request: request.clone(),
            rights_scope: scope,
            review_key: key,
            review_revision: review.revision,
            agreement: EvidenceId::new(&review.agreement).map_err(|_| invalid())?,
            checked_at_ms: checked_at,
            expires_at_ms: review.expires_at_ms.min(retention_until),
            review_until_ms: review.expires_at_ms,
            retention_until_ms: retention_until,
        };
        permit.bind(request, checked_at).map_err(|_| denied())?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(permit)
    }
}
