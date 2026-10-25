//! Scope -> account -> request lock order. No storage/provider I/O inside transactions.
use crate::{PostgresAcquisitionLedger, ledger};
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::{
    AccountBudget, AdmissionRecord, BudgetError as Error, BudgetErrorKind as K, CleanupAuthority,
    EvidenceAccess, EvidenceCleanup, EvidenceRetention, NativeEvidenceRetention, RetentionScope,
};
use sqlx::{Postgres, Transaction};

pub(super) fn scope_key(scope: &RetentionScope) -> String {
    format!(
        "{}/{}/{}",
        scope.storage.as_str(),
        scope.audience.as_str(),
        scope.rights.as_str()
    )
}
fn object_key(o: &EvidenceObject) -> String {
    format!(
        "{}/{}/{}/{}/{}",
        o.storage.as_str(),
        o.audience.as_str(),
        o.rights.id.as_str(),
        o.rights.revision.as_str(),
        o.sha256.as_str()
    )
}
fn invalid(_: impl std::fmt::Debug) -> Error {
    Error::new(K::Invalid)
}
pub(super) async fn registered(
    tx: &mut Transaction<'_, Postgres>,
    record: &AdmissionRecord,
    object: &EvidenceObject,
) -> Result<(), Error> {
    let key = object_key(object);
    let row = sqlx::query!(
        "SELECT descriptor,request_id FROM edgeagent_evidence_objects WHERE object_key=$1",
        key
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(Error::storage)?
    .ok_or_else(|| Error::new(K::Denied))?;
    let stored: EvidenceObject = serde_json::from_slice(&row.descriptor).map_err(invalid)?;
    if row.request_id != record.command().request.as_str() || stored != *object {
        return Err(Error::new(K::Denied));
    }
    Ok(())
}
pub(super) async fn not_retired(
    tx: &mut Transaction<'_, Postgres>,
    id: &EvidenceId,
) -> Result<(), Error> {
    let denied = sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_acquisition_denials WHERE request_id=$1) AS "denied!""#,id.as_str())
        .fetch_one(&mut **tx).await.map_err(Error::storage)?;
    if denied {
        Err(Error::new(K::Denied))
    } else {
        Ok(())
    }
}
async fn lock_scope(tx: &mut Transaction<'_, Postgres>, scope: &str) -> Result<(), Error> {
    let row = sqlx::query!(
        "SELECT revoked FROM edgeagent_evidence_scopes WHERE scope=$1 FOR SHARE",
        scope
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(Error::storage)?
    .ok_or_else(|| Error::new(K::Denied))?;
    if row.revoked {
        Err(Error::new(K::Denied))
    } else {
        Ok(())
    }
}
pub(super) async fn active(
    tx: &mut Transaction<'_, Postgres>,
    record: &AdmissionRecord,
) -> Result<(), Error> {
    lock_scope(tx, &scope_key(&record.retention_scope()?)).await
}
pub(super) async fn guard_request(
    tx: &mut Transaction<'_, Postgres>,
    id: &EvidenceId,
) -> Result<(), Error> {
    not_retired(tx, id).await?;
    let scope = sqlx::query!(
        r#"SELECT retention_scope,delete_ms,floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!" FROM edgeagent_acquisitions WHERE request_id=$1"#,
        id.as_str()
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(Error::storage)?;
    if let Some(scope) = scope {
        if scope.now >= scope.delete_ms {
            return Err(Error::new(K::Denied));
        }
        lock_scope(tx, &scope.retention_scope).await?;
    }
    Ok(())
}

impl PostgresAcquisitionLedger {
    async fn stage_object(
        &self,
        command: AcquireEvidence,
        object: &EvidenceObject,
        verify: impl FnOnce(&AdmissionRecord) -> Result<(), Error> + Send,
        preparation: Option<&NativeDailyArtifact>,
    ) -> Result<(), Error> {
        let mut tx = self.transaction().await?;
        let (record, _) = ledger::load(&mut tx, &command).await?;
        verify(&record)?;
        let until = record.valid_until_ms()?;
        let valid = sqlx::query_scalar!(
            r#"SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint < $1 AS "valid!""#,
            until
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        if !valid {
            return Err(Error::new(K::Deadline));
        }
        let key = object_key(object);
        let bytes = serde_json::to_vec(object).map_err(invalid)?;
        let count = sqlx::query_scalar!(
            r#"SELECT count(*) AS "count!" FROM edgeagent_evidence_objects WHERE request_id=$1"#,
            command.request.as_str()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        let existing = sqlx::query!(
            "SELECT descriptor FROM edgeagent_evidence_objects WHERE object_key=$1",
            key
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::storage)?;
        if let Some(existing) = existing {
            if existing.descriptor != bytes {
                return Err(Error::new(K::Conflict));
            }
        } else {
            if count >= 12 {
                return Err(Error::new(K::Exhausted));
            }
            sqlx::query!("INSERT INTO edgeagent_evidence_objects(object_key,request_id,descriptor) VALUES($1,$2,$3)",key,command.request.as_str(),bytes)
                .execute(&mut *tx).await.map_err(Error::storage)?;
        }
        if let Some(artifact) = preparation {
            sqlx::query!("INSERT INTO edgeagent_native_preparations(request_id,digest,artifact) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",command.request.as_str(),artifact.digest().as_str(),artifact.bytes())
                .execute(&mut *tx).await.map_err(Error::storage)?;
            let retained = sqlx::query!(
                "SELECT digest,artifact FROM edgeagent_native_preparations WHERE request_id=$1",
                command.request.as_str()
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(Error::storage)?;
            if retained.digest != artifact.digest().as_str()
                || retained.artifact != artifact.bytes()
            {
                return Err(Error::new(K::Conflict));
            }
        }
        tx.commit().await.map_err(Error::storage)
    }
}
impl EvidenceRetention for PostgresAcquisitionLedger {
    async fn stage(&self, artifact: &DailyEvidenceArtifact) -> Result<(), Error> {
        let data = artifact.data();
        let command = AcquireEvidence {
            request: data.scope.request.clone(),
            request_sha256: data.scope.request_sha256.clone(),
        };
        let decision = EvidenceDecision::ready(command.clone(), artifact).map_err(invalid)?;
        let EvidenceOutcome::Ready { object } = &decision.data().outcome else {
            return Err(Error::new(K::Invalid));
        };
        self.stage_object(command, object, |r| r.verify_artifact(artifact), None)
            .await
    }
    async fn check(&self, object: &EvidenceObject) -> Result<(), Error> {
        let mut tx = self.transaction().await?;
        let key = object_key(object);
        let row=sqlx::query!(r#"
            SELECT o.descriptor,a.delete_ms,s.revoked,
              floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!",
              (EXISTS(SELECT 1 FROM edgeagent_acquisition_receipts r WHERE r.request_id=a.request_id)
                OR a.orphan_ms>floor(extract(epoch FROM clock_timestamp())*1000)::bigint) AS "usable!"
            FROM edgeagent_evidence_objects o JOIN edgeagent_acquisitions a USING(request_id)
            JOIN edgeagent_evidence_scopes s ON s.scope=a.retention_scope WHERE o.object_key=$1
        "#,key).fetch_optional(&mut *tx).await.map_err(Error::storage)?.ok_or_else(||Error::new(K::Denied))?;
        let stored: EvidenceObject = serde_json::from_slice(&row.descriptor).map_err(invalid)?;
        let expiry = i128::from(object.delete_by.seconds) * 1000
            + i128::from(object.delete_by.nanos / 1_000_000);
        if stored != *object
            || row.revoked
            || !row.usable
            || row.now >= row.delete_ms
            || i128::from(row.now) >= expiry
        {
            return Err(Error::new(K::Denied));
        }
        tx.commit().await.map_err(Error::storage)
    }
    async fn pending_cleanup(&self, limit: u8) -> Result<Vec<EvidenceCleanup>, Error> {
        if !(1..=16).contains(&limit) {
            return Err(Error::new(K::Invalid));
        }
        let limit = i64::from(limit);
        let mut tx = self.transaction().await?;
        let ids=sqlx::query!(r#"WITH chosen AS (
            SELECT request_id FROM edgeagent_acquisition_denials WHERE NOT cleaned
            ORDER BY cleanup_order LIMIT $1 FOR UPDATE SKIP LOCKED)
            UPDATE edgeagent_acquisition_denials d SET cleanup_order=nextval('edgeagent_evidence_cleanup_order')
            FROM chosen WHERE d.request_id=chosen.request_id RETURNING d.request_id"#,limit)
            .fetch_all(&mut *tx).await.map_err(Error::storage)?;
        let mut batch = Vec::new();
        for id in ids {
            let rows=sqlx::query!("SELECT descriptor FROM edgeagent_evidence_objects WHERE request_id=$1 ORDER BY object_key LIMIT 13",id.request_id)
                .fetch_all(&mut *tx).await.map_err(Error::storage)?;
            if rows.len() > 12 {
                return Err(Error::new(K::Invalid));
            }
            let objects = rows
                .into_iter()
                .map(|r| serde_json::from_slice(&r.descriptor).map_err(invalid))
                .collect::<Result<Vec<_>, _>>()?;
            batch.push(EvidenceCleanup {
                request: EvidenceId::new(&id.request_id).map_err(invalid)?,
                objects,
            });
        }
        tx.commit().await.map_err(Error::storage)?;
        Ok(batch)
    }
    async fn complete_cleanup(&self, id: &EvidenceId) -> Result<(), Error> {
        let mut tx = self.transaction().await?;
        let retired = sqlx::query!(
            "SELECT request_id FROM edgeagent_acquisition_denials WHERE request_id=$1 FOR UPDATE",
            id.as_str()
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::storage)?;
        if retired.is_none() {
            return Err(Error::new(K::Denied));
        }
        sqlx::query!(
            "DELETE FROM edgeagent_evidence_objects WHERE request_id=$1",
            id.as_str()
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        sqlx::query!(
            "UPDATE edgeagent_acquisition_denials SET cleaned=TRUE WHERE request_id=$1",
            id.as_str()
        )
        .execute(&mut *tx)
        .await
        .map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)
    }
}
impl NativeEvidenceRetention for PostgresAcquisitionLedger {
    async fn stage_native(&self, artifact: &NativeDailyArtifact) -> Result<(), Error> {
        let scope = &artifact.data().capture.scope;
        let command = AcquireEvidence {
            request: scope.request.clone(),
            request_sha256: scope.request_sha256.clone(),
        };
        self.stage_object(
            command,
            &artifact.object().map_err(invalid)?,
            |r| r.verify_native_artifact(artifact),
            Some(artifact),
        )
        .await
    }
    async fn recover_native(
        &self,
        command: &AcquireEvidence,
        access: &impl EvidenceAccess,
    ) -> Result<Option<NativeDailyArtifact>, Error> {
        let mut tx = self.transaction().await?;
        let (record, _) = ledger::load(&mut tx, command).await?;
        let row = sqlx::query!(
            "SELECT digest,artifact FROM edgeagent_native_preparations WHERE request_id=$1",
            command.request.as_str()
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::storage)?;
        let artifact = row
            .map(|r| {
                NativeDailyArtifact::decode(
                    &r.artifact,
                    &EvidenceDigest::new(&r.digest).map_err(invalid)?,
                )
                .map_err(invalid)
            })
            .transpose()?;
        if let Some(a) = &artifact {
            record.verify_native_artifact(a)?;
            let object = a.object().map_err(invalid)?;
            access
                .authorize(&object)
                .map_err(|e| Error::with_source(K::Denied, e))?;
            let now = i128::from(access.now().unix_millis());
            if now
                >= i128::from(object.delete_by.seconds) * 1000
                    + i128::from(object.delete_by.nanos / 1_000_000)
            {
                return Err(Error::new(K::Deadline));
            }
        }
        crate::retention::guard_request(&mut tx, &command.request).await?;
        tx.commit().await.map_err(Error::storage)?;
        if let Some(a) = &artifact {
            let object = a.object().map_err(invalid)?;
            self.check(&object).await?;
            access
                .authorize(&object)
                .map_err(|e| Error::with_source(K::Denied, e))?;
            if i128::from(access.now().unix_millis())
                >= i128::from(object.delete_by.seconds) * 1000
                    + i128::from(object.delete_by.nanos / 1_000_000)
            {
                return Err(Error::new(K::Deadline));
            }
        }
        Ok(artifact)
    }
}
impl PostgresAcquisitionLedger {
    /// Irreversibly withdraw one licensed namespace. Already-returned bytes cannot be recalled.
    /// No cleanup/quiescence wait delays denial; future reuse requires a new rights identity.
    /// # Errors
    /// Returns Storage on uncertain commit; retry the same namespace, never assume rollback.
    pub async fn revoke_evidence_scope(&self, scope: &RetentionScope) -> Result<(), Error> {
        let key = scope_key(scope);
        let mut tx = self.transaction().await?;
        sqlx::query!("INSERT INTO edgeagent_evidence_scopes(scope,revoked) VALUES($1,TRUE) ON CONFLICT(scope) DO UPDATE SET revoked=TRUE",key)
            .execute(&mut *tx).await.map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)
    }
    /// Retire at most sixteen due commands, committing denial and metadata erasure per command.
    /// Unpublished objects become orphans only after the original command deadline. Ready objects
    /// remain retained until licensed expiry/revocation. Unknown provider calls become opaque blocks.
    /// # Errors
    /// Storage/lock failure leaves committed earlier retirements intact; rerun this bounded batch.
    pub async fn retire_due_evidence(&self, limit: u8) -> Result<usize, Error> {
        if !(1..=16).contains(&limit) {
            return Err(Error::new(K::Invalid));
        }
        let limit = i64::from(limit);
        let mut tx = self.transaction().await?;
        let ids=sqlx::query!(r#"
            SELECT a.request_id FROM edgeagent_acquisitions a JOIN edgeagent_evidence_scopes s ON s.scope=a.retention_scope
            WHERE s.revoked OR a.delete_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint
            OR (a.orphan_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint
                AND NOT EXISTS(SELECT 1 FROM edgeagent_acquisition_receipts r WHERE r.request_id=a.request_id))
            ORDER BY a.request_id LIMIT $1
        "#,limit).fetch_all(&mut *tx).await.map_err(Error::storage)?;
        tx.commit().await.map_err(Error::storage)?;
        let mut count = 0;
        for id in ids {
            if self.retire(&id.request_id).await? {
                count += 1;
            }
        }
        Ok(count)
    }
    async fn retire(&self, id: &str) -> Result<bool, Error> {
        let mut tx = self.transaction().await?;
        let owner = sqlx::query!(
            "SELECT account_id,retention_scope FROM edgeagent_acquisitions WHERE request_id=$1",
            id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::storage)?;
        let Some(owner) = owner else {
            return Ok(false);
        };
        sqlx::query!(
            "SELECT scope FROM edgeagent_evidence_scopes WHERE scope=$1 FOR UPDATE",
            owner.retention_scope
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        let account = sqlx::query!(
            "SELECT budget FROM edgeagent_acquisition_accounts WHERE account_id=$1 FOR UPDATE",
            owner.account_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::storage)?;
        let due=sqlx::query!(r#"
            SELECT a.request_id FROM edgeagent_acquisitions a JOIN edgeagent_evidence_scopes s ON s.scope=a.retention_scope
            WHERE a.request_id=$1 AND (s.revoked OR a.delete_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint
              OR (a.orphan_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint
                AND NOT EXISTS(SELECT 1 FROM edgeagent_acquisition_receipts r WHERE r.request_id=a.request_id))) FOR UPDATE OF a
        "#,id).fetch_optional(&mut *tx).await.map_err(Error::storage)?;
        if due.is_none() {
            return Ok(false);
        }
        let mut budget = AccountBudget::decode(&account.budget)?;
        let blocked = budget.retire_pending(&EvidenceId::new(id).map_err(invalid)?);
        let bytes = budget.encode()?;
        sqlx::query!("UPDATE edgeagent_acquisition_accounts SET budget=$2,blocked=blocked OR $3,block_generation=block_generation+CASE WHEN $3 THEN 1 ELSE 0 END WHERE account_id=$1",owner.account_id,bytes,blocked)
            .execute(&mut *tx).await.map_err(Error::storage)?;
        sqlx::query!("INSERT INTO edgeagent_acquisition_denials(request_id) VALUES($1) ON CONFLICT DO NOTHING",id)
            .execute(&mut *tx).await.map_err(Error::storage)?;
        erase_metadata(&mut tx, id).await?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(true)
    }
    /// Inspect an opaque current account block generation. This is not quiescence evidence.
    /// # Errors
    /// Returns NotFound for an unknown account, Storage for database failure.
    pub async fn account_block(&self, account: &EvidenceId) -> Result<Option<i64>, Error> {
        let mut tx = self.transaction().await?;
        let row=sqlx::query!("SELECT blocked,block_generation FROM edgeagent_acquisition_accounts WHERE account_id=$1",account.as_str())
            .fetch_optional(&mut *tx).await.map_err(Error::storage)?.ok_or_else(||Error::new(K::NotFound))?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(row.blocked.then_some(row.block_generation))
    }
    /// Clear only the exact opaque block generation after owner-confirmed provider quiescence.
    /// Never recreates deleted commands, refunds charges or resets account rate/retry history.
    /// # Errors
    /// Denies absent confirmation; Conflict protects a newer generation from stale completion.
    pub async fn confirm_retired_account(
        &self,
        account: &EvidenceId,
        generation: i64,
        owner: &impl CleanupAuthority,
    ) -> Result<(), Error> {
        if generation <= 0 {
            return Err(Error::new(K::Invalid));
        }
        owner.confirm_account_quiescent(account, generation)?;
        let mut tx = self.transaction().await?;
        // A worker-provided trait implementation cannot manufacture owner confirmation.
        crate::qualification::require_owner(&mut tx).await?;
        let row = sqlx::query!(
            "SELECT budget,blocked FROM edgeagent_acquisition_accounts WHERE account_id=$1 FOR UPDATE",
            account.as_str()
        ).fetch_optional(&mut *tx).await.map_err(Error::storage)?
            .ok_or_else(|| Error::new(K::NotFound))?;
        if row.blocked {
            let mut shared = AccountBudget::decode(&row.budget)?;
            let now = sqlx::query_scalar!(
                r#"SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS "now!""#
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(Error::storage)?;
            shared.confirm_retired_daily_credits(now);
            let state = shared.encode()?;
            sqlx::query!(
                "UPDATE edgeagent_acquisition_accounts SET budget=$2 WHERE account_id=$1",
                account.as_str(),
                state
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::storage)?;
        }
        let result=sqlx::query!("UPDATE edgeagent_acquisition_accounts SET blocked=FALSE WHERE account_id=$1 AND block_generation=$2",account.as_str(),generation)
            .execute(&mut *tx).await.map_err(Error::storage)?;
        if result.rows_affected() != 1 {
            return Err(Error::new(K::Conflict));
        }
        tx.commit().await.map_err(Error::storage)
    }
}

async fn erase_metadata(tx: &mut Transaction<'_, Postgres>, id: &str) -> Result<(), Error> {
    let ownership=sqlx::query!("DELETE FROM edgeagent_evidence_ownership WHERE request_id=$1 RETURNING series_key,observations",id)
        .fetch_optional(&mut **tx).await.map_err(Error::storage)?;
    if let Some(o) = ownership {
        sqlx::query!(
            "DELETE FROM edgeagent_evidence_heads WHERE series_key=$1 AND owner_request=$2",
            o.series_key,
            id
        )
        .execute(&mut **tx)
        .await
        .map_err(Error::storage)?;
        sqlx::query!("DELETE FROM edgeagent_observation_versions v WHERE identity=ANY($1) AND NOT EXISTS(SELECT 1 FROM edgeagent_evidence_ownership o WHERE v.identity=ANY(o.observations))",&o.observations)
            .execute(&mut **tx).await.map_err(Error::storage)?;
    }
    sqlx::query!(
        "DELETE FROM edgeagent_acquisition_receipts WHERE request_id=$1",
        id
    )
    .execute(&mut **tx)
    .await
    .map_err(Error::storage)?;
    sqlx::query!("DELETE FROM edgeagent_message_inbox WHERE consumer_name='market-data-acquisition' AND convert_from(envelope,'UTF8')::jsonb #>> '{data,request}'=$1",id)
        .execute(&mut **tx).await.map_err(Error::storage)?;
    sqlx::query!("DELETE FROM edgeagent_message_outbox WHERE message_source='urn:edgeagent:component:market-data' AND message_id='evidence-decision-'||$1",id)
        .execute(&mut **tx).await.map_err(Error::storage)?;
    sqlx::query!("DELETE FROM edgeagent_acquisitions WHERE request_id=$1", id)
        .execute(&mut **tx)
        .await
        .map_err(Error::storage)?;
    Ok(())
}
