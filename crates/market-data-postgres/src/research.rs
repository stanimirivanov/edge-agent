//! Durable MarketData-owned research/acquisition correlation; no peer tables or external I/O.
use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    evidence::{AcquireEvidence, EvidenceDigest, EvidenceId},
    research::{ResearchRecord, ResearchRequest},
    research_acquisition::{self, DECIDED, ResearchEvidence},
};
use edgeagent_inbox_postgres::PostgresInbox;
use edgeagent_market_data_application::{
    AdmissionRecord, BudgetError as Error, BudgetErrorKind as K, TerminalReceipt,
};
use edgeagent_outbox_postgres::{EnqueueDisposition, PostgresOutbox};
/// Retained owner-selected work. A completed publication is historical metadata, not read authority.
pub struct PendingResearchEvidence {
    /// Exact original research intent.
    pub request: ResearchRequest,
    /// Original ingress context, retained across duplicate/reordered deliveries.
    pub delivery: MessageEnvelope,
    /// Installed owner admission, never client-provided execution configuration.
    pub admission: AdmissionRecord,
    /// First committed result; None is pending work that must resume.
    pub publication: Option<MessageEnvelope>,
}
/// Caller supplies a MarketData-private pool; migration/activation remain explicit.
pub struct PostgresResearchEvidence {
    pool: sqlx::PgPool,
}
fn invalid() -> Error {
    Error::new(K::Invalid)
}
impl PostgresResearchEvidence {
    /// Bind the owner pool without I/O.
    #[must_use]
    pub const fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
    /// Resolve only an operator-selected existing owner admission identity.
    /// # Errors
    /// Rejects absent/corrupt owner state or storage failure.
    pub async fn owner_record(&self, id: &EvidenceId) -> Result<AdmissionRecord, Error> {
        let row = sqlx::query!(
            "SELECT admission,digest FROM edgeagent_acquisitions WHERE request_id=$1",
            id.as_str()
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Error::storage)?;
        let command = AcquireEvidence {
            request: id.clone(),
            request_sha256: EvidenceDigest::new(&row.digest).map_err(|_| invalid())?,
        };
        AdmissionRecord::decode(&row.admission, &command)
    }
    /// Check an opaque retained scope/request denial; it contains no licensed fingerprints.
    /// # Errors
    /// Rejects malformed ingress or unavailable storage.
    pub async fn denied(&self, delivery: &MessageEnvelope) -> Result<bool, Error> {
        let request = research_acquisition::request(delivery).map_err(|_| invalid())?;
        sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_research_evidence_denials WHERE caller_scope=$1 AND request_id=$2) AS "denied!""#,request.scope,request.request_id).fetch_one(&self.pool).await.map_err(Error::storage)
    }
    /// Authenticate a retained correlated publication before an owner-controlled object read.
    /// # Errors
    /// Rejects erased/pending/conflicting publication or corrupt retained outbox state.
    pub async fn export(&self, event: &MessageEnvelope) -> Result<PendingResearchEvidence, Error> {
        let outcome = ResearchEvidence::from_envelope(event).map_err(|_| invalid())?;
        let request = outcome.request();
        let mut tx = self.tx().await?;
        Self::fence(&mut tx, request).await?;
        let retained = Self::read(&mut tx, request).await?;
        if retained
            .publication
            .as_ref()
            .ok_or_else(|| Error::new(K::Denied))?
            .to_json()
            .map_err(|_| invalid())?
            != event.to_json().map_err(|_| invalid())?
        {
            return Err(Error::new(K::Conflict));
        }
        tx.commit().await.map_err(Error::storage)?;
        Ok(retained)
    }
    async fn read(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        request: &ResearchRequest,
    ) -> Result<PendingResearchEvidence, Error> {
        let row=sqlx::query!("SELECT request,delivery,acquisition_id,acquisition_digest,admission,publication FROM edgeagent_research_evidence WHERE caller_scope=$1 AND request_id=$2 FOR UPDATE",request.scope,request.request_id).fetch_one(&mut **tx).await.map_err(Error::storage)?;
        if row.request
            != ResearchRecord::Request(request.clone())
                .canonical_bytes()
                .map_err(|_| invalid())?
        {
            return Err(Error::new(K::Conflict));
        }
        let delivery = MessageEnvelope::from_json(&row.delivery).map_err(|_| invalid())?;
        if research_acquisition::request(&delivery).map_err(|_| invalid())? != *request {
            return Err(invalid());
        }
        let command = AcquireEvidence {
            request: EvidenceId::new(&row.acquisition_id).map_err(|_| invalid())?,
            request_sha256: EvidenceDigest::new(&row.acquisition_digest).map_err(|_| invalid())?,
        };
        let admission = AdmissionRecord::decode(&row.admission, &command)?;
        let publication = if let Some(bytes) = row.publication {
            let envelope = MessageEnvelope::from_json(&bytes).map_err(|_| invalid())?;
            let outcome = ResearchEvidence::from_envelope(&envelope).map_err(|_| invalid())?;
            if outcome.request() != request || envelope.to_json().map_err(|_| invalid())? != bytes {
                return Err(invalid());
            }
            Self::intact(tx, &envelope, &bytes, DECIDED).await?;
            Some(envelope)
        } else {
            None
        };
        Ok(PendingResearchEvidence {
            request: request.clone(),
            delivery,
            admission,
            publication,
        })
    }
    async fn intact(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        event: &MessageEnvelope,
        bytes: &[u8],
        definition: edgeagent_contracts::MessageDefinition,
    ) -> Result<(), Error> {
        let subject = definition.subject().map_err(|_| invalid())?;
        let intact=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_message_outbox WHERE message_source=$1 AND message_id=$2 AND envelope=$3 AND message_type=$4 AND transport_subject=$5) AS "intact!""#,event.source(),event.id(),bytes,definition.message_type(),subject).fetch_one(&mut **tx).await.map_err(Error::storage)?;
        if !intact {
            return Err(invalid());
        }
        Ok(())
    }
    async fn fence(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        request: &ResearchRequest,
    ) -> Result<(), Error> {
        let key = format!(
            "edgeagent.research-evidence/{}/{}",
            request.scope, request.request_id
        );
        sqlx::query!("SELECT pg_advisory_xact_lock(hashtextextended($1,0))", key)
            .execute(&mut **tx)
            .await
            .map_err(Error::storage)?;
        let denied=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_research_evidence_denials WHERE caller_scope=$1 AND request_id=$2) AS "denied!""#,request.scope,request.request_id).fetch_one(&mut **tx).await.map_err(Error::storage)?;
        if denied {
            return Err(Error::new(K::Denied));
        }
        Ok(())
    }
    async fn tx(&self) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, Error> {
        let mut tx = self.pool.begin().await.map_err(Error::storage)?;
        sqlx::query!("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await
            .map_err(Error::storage)?;
        sqlx::query!("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *tx)
            .await
            .map_err(Error::storage)?;
        sqlx::query!("SET LOCAL statement_timeout = '15s'")
            .execute(&mut *tx)
            .await
            .map_err(Error::storage)?;
        Ok(tx)
    }
    /// Retain the first admitted owner plan before any acquisition work; retries resume it.
    /// # Errors
    /// Rejects foreign routing, changed scoped intent, unknown admission or storage/integrity failure.
    pub async fn reserve(
        &self,
        delivery: &MessageEnvelope,
        record: &AdmissionRecord,
    ) -> Result<PendingResearchEvidence, Error> {
        let request = research_acquisition::request(delivery).map_err(|_| invalid())?;
        if self.denied(delivery).await? {
            return Err(Error::new(K::Denied));
        }
        let bytes = ResearchRecord::Request(request.clone())
            .canonical_bytes()
            .map_err(|_| invalid())?;
        let original = delivery.to_json().map_err(|_| invalid())?;
        let mut tx = self.tx().await?;
        Self::fence(&mut tx, &request).await?;
        let exists=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM edgeagent_acquisitions WHERE request_id=$1 AND digest=$2 AND admission=$3) AS "exists!""#,record.command().request.as_str(),record.command().request_sha256.as_str(),record.bytes()).fetch_one(&mut *tx).await.map_err(Error::storage)?;
        if !exists {
            return Err(invalid());
        }
        sqlx::query!("INSERT INTO edgeagent_research_evidence(caller_scope,request_id,request,delivery,acquisition_id,acquisition_digest,admission) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(caller_scope,request_id) DO NOTHING",request.scope,request.request_id,bytes,original,record.command().request.as_str(),record.command().request_sha256.as_str(),record.bytes()).execute(&mut *tx).await.map_err(Error::storage)?;
        let pending = Self::read(&mut tx, &request).await?;
        if pending.delivery.source() == delivery.source()
            && pending.delivery.id() == delivery.id()
            && pending.delivery.to_json().map_err(|_| invalid())?
                != delivery.to_json().map_err(|_| invalid())?
        {
            return Err(Error::new(K::Conflict));
        }
        if pending.publication.is_some() {
            Self::inbox(&mut tx, delivery).await?;
        }
        tx.commit().await.map_err(Error::storage)?;
        Ok(pending)
    }
    async fn inbox(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        delivery: &MessageEnvelope,
    ) -> Result<(), Error> {
        PostgresInbox
            .record_delivery(
                tx,
                "market-data-research",
                &research_acquisition::registry().map_err(|_| invalid())?,
                delivery,
            )
            .await
            .map_err(|e| {
                let kind = match e.kind() {
                    edgeagent_inbox_postgres::InboxErrorKind::Storage => K::Storage,
                    edgeagent_inbox_postgres::InboxErrorKind::MessageIdentityConflict => {
                        K::Conflict
                    }
                    _ => K::Invalid,
                };
                Error::with_source(kind, e)
            })?;
        Ok(())
    }
    /// Commit the first correlated outcome with inbox/outbox; ready requires an existing owner receipt.
    /// # Errors
    /// Rejects wrong request/acquisition correlation, fabricated ready references or integrity/storage failures.
    pub async fn complete(
        &self,
        delivery: &MessageEnvelope,
        outcome: &ResearchEvidence,
    ) -> Result<MessageEnvelope, Error> {
        let request = research_acquisition::request(delivery).map_err(|_| invalid())?;
        if self.denied(delivery).await? {
            return Err(Error::new(K::Denied));
        }
        outcome.validate().map_err(|_| invalid())?;
        if outcome.request() != &request {
            return Err(Error::new(K::Conflict));
        }
        let mut tx = self.tx().await?;
        Self::fence(&mut tx, &request).await?;
        let pending = Self::read(&mut tx, &request).await?;
        if pending.delivery.source() == delivery.source()
            && pending.delivery.id() == delivery.id()
            && pending.delivery.to_json().map_err(|_| invalid())?
                != delivery.to_json().map_err(|_| invalid())?
        {
            return Err(Error::new(K::Conflict));
        }
        if let Some(event) = pending.publication {
            Self::inbox(&mut tx, delivery).await?;
            tx.commit().await.map_err(Error::storage)?;
            return Ok(event);
        }
        if let ResearchEvidence::Ready { decision, .. } = outcome {
            if &decision.request != pending.admission.command() {
                return Err(Error::new(K::Conflict));
            }
            let receipt = sqlx::query_scalar!(
                "SELECT event FROM edgeagent_acquisition_receipts WHERE request_id=$1",
                decision.request.request.as_str()
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(Error::storage)?;
            let receipt = TerminalReceipt::decode(&receipt, pending.admission.command())?;
            Self::intact(
                &mut tx,
                receipt.envelope(),
                receipt.bytes(),
                edgeagent_contracts::evidence::DEFINITIONS[1],
            )
            .await?;
            if receipt.decision().data() != decision.as_ref() {
                return Err(Error::new(K::Conflict));
            }
        }
        let (key, partition) = research_acquisition::binding(&request).map_err(|_| invalid())?;
        let time=sqlx::query_scalar!(r#"SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS "time!""#).fetch_one(&mut *tx).await.map_err(Error::storage)?;
        let event = MessageEnvelope::from_payload(
            MessageMetadata {
                id: format!("research-evidence-{key}"),
                source: Component::MarketData.source_uri().into(),
                message_type: DECIDED.message_type().into(),
                subject: partition.clone(),
                time,
                data_schema: DECIDED.data_schema().into(),
                correlation_id: key.clone(),
                causation_id: pending.delivery.id().into(),
                idempotency_key: key,
                partition_key: partition,
                trace_parent: pending
                    .delivery
                    .extension("traceparent")
                    .ok_or_else(invalid)?
                    .into(),
                trace_state: pending.delivery.extension("tracestate").map(str::to_owned),
            },
            outcome,
        )
        .map_err(|_| invalid())?;
        let bytes = event.to_json().map_err(|_| invalid())?;
        sqlx::query!("UPDATE edgeagent_research_evidence SET publication=$3 WHERE caller_scope=$1 AND request_id=$2 AND publication IS NULL",request.scope,request.request_id,bytes).execute(&mut *tx).await.map_err(Error::storage)?;
        Self::inbox(&mut tx, delivery).await?;
        if PostgresOutbox
            .enqueue(&mut tx, DECIDED, &event)
            .await
            .map_err(|e| Error::with_source(K::Storage, e))?
            != EnqueueDisposition::Inserted
        {
            return Err(invalid());
        }
        tx.commit().await.map_err(Error::storage)?;
        Ok(event)
    }
}
