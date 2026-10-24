//! Shared invented ready-publication inputs for model and PostgreSQL proofs.
use edgeagent_contracts::{Component, MessageEnvelope, MessageMetadata, evidence::*};
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub struct Access {
    now: UtcInstant,
    pub denied: AtomicBool,
}
impl Access {
    pub fn new(now: i64) -> std::result::Result<Self, ValueError> {
        Ok(Self {
            now: UtcInstant::from_unix_millis(now)?,
            denied: AtomicBool::new(false),
        })
    }
}
impl EvidenceAccess for Access {
    fn now(&self) -> UtcInstant {
        self.now
    }
    fn authorize(&self, _: &EvidenceObject) -> std::result::Result<(), EvidenceStorageError> {
        if self.denied.load(Ordering::SeqCst) {
            Err(EvidenceStorageError::new(EvidenceStorageErrorKind::Denied))
        } else {
            Ok(())
        }
    }
}
impl PublicationAuthority for Access {
    fn authorize_publication(
        &self,
        _: &AdmissionRecord,
        object: &EvidenceObject,
    ) -> std::result::Result<(), BudgetError> {
        self.authorize(object)
            .map_err(|e| BudgetError::with_source(BudgetErrorKind::Denied, e))
    }
}
#[derive(Default)]
pub struct Storage {
    pub objects: Mutex<Vec<DailyEvidenceArtifact>>,
    pub calls: AtomicUsize,
    pub fail: AtomicBool,
}
impl EvidenceStorage for Storage {
    async fn put(
        &self,
        artifact: &DailyEvidenceArtifact,
        access: &impl EvidenceAccess,
    ) -> std::result::Result<EvidenceObject, EvidenceStorageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(EvidenceStorageError::new(EvidenceStorageErrorKind::Backend));
        }
        self.objects
            .lock()
            .map_err(|_| EvidenceStorageError::new(EvidenceStorageErrorKind::Backend))?
            .push(artifact.clone());
        let command = AcquireEvidence {
            request: artifact.data().scope.request.clone(),
            request_sha256: artifact.data().scope.request_sha256.clone(),
        };
        let decision = EvidenceDecision::ready(command, artifact)
            .map_err(|_| EvidenceStorageError::new(EvidenceStorageErrorKind::Invalid))?;
        let EvidenceOutcome::Ready { object } = decision.data().outcome.clone() else {
            return Err(EvidenceStorageError::new(EvidenceStorageErrorKind::Invalid));
        };
        access.authorize(&object)?;
        Ok(object)
    }
    async fn get(
        &self,
        object: &EvidenceObject,
        access: &impl EvidenceAccess,
    ) -> std::result::Result<DailyEvidenceArtifact, EvidenceStorageError> {
        access.authorize(object)?;
        self.objects
            .lock()
            .map_err(|_| EvidenceStorageError::new(EvidenceStorageErrorKind::Backend))?
            .iter()
            .find(|a| a.digest() == &object.sha256)
            .cloned()
            .ok_or_else(|| EvidenceStorageError::new(EvidenceStorageErrorKind::NotFound))
    }
}
pub fn delivery(
    command: &AcquireEvidence,
    id: &str,
) -> std::result::Result<MessageEnvelope, edgeagent_contracts::MessageContractError> {
    let definition = DEFINITIONS[0];
    MessageEnvelope::from_payload(
        MessageMetadata {
            id: id.into(),
            source: Component::Gateway.source_uri().into(),
            message_type: definition.message_type().into(),
            subject: format!("acquisition/{}", command.request.as_str()),
            time: "2026-10-23T00:00:00Z".into(),
            data_schema: definition.data_schema().into(),
            correlation_id: "synthetic-acquisition".into(),
            causation_id: "synthetic-parent".into(),
            idempotency_key: command.request.as_str().into(),
            partition_key: format!("acquisition/{}", command.request.as_str()),
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
            trace_state: None,
        },
        command,
    )
}
pub async fn prepare(
    record: &AdmissionRecord,
    admitted: &AuthorizedAcquisition<'_>,
    catalogs: AuthorityCatalogs<'_>,
    storage: &impl EvidenceStorage,
    access: &Access,
    previous: Option<&DailyEvidenceArtifact>,
    price: i128,
) -> Result<TerminalProposal> {
    let r = record.request();
    let now = access.now();
    let day = SessionDate::new(1970, 1, 1)?;
    let symbol = Symbol::new("INVENTED")?;
    let capability = admitted.authority().1.value().capability;
    let ver = |id| -> Result<(ReferenceId, ReferenceRevision, SourceId)> {
        Ok((
            ReferenceId::new(id)?,
            ReferenceRevision::new(1)?,
            SourceId::new(9)?,
        ))
    };
    let (id, rev, source) = ver(1)?;
    let mappings = ReferenceHistory::new(vec![ReferenceVersion::new(
        id,
        rev,
        source,
        now,
        [7; 32],
        InstrumentMapping::new(
            r.scope.instrument,
            ListingId::new(1)?,
            symbol.clone(),
            Venue::Xnys,
            r.range,
        ),
    )])?;
    let calendar = ReferenceHistory::new(vec![ReferenceVersion::new(
        id,
        rev,
        source,
        now,
        [7; 32],
        CalendarDay::new(
            Venue::Xnys,
            day,
            SessionHours::Regular(SessionWindow::new(
                UtcInstant::from_unix_millis(14 * 3_600_000)?,
                UtcInstant::from_unix_millis(21 * 3_600_000)?,
            )?),
            "synthetic",
        )?,
    )])?;
    let actions = ReferenceHistory::new(vec![])?;
    let coverage = ReferenceVersion::new(
        id,
        rev,
        source,
        now,
        [7; 32],
        ActionCoverage::new(
            r.scope.instrument,
            r.range,
            UtcInstant::from_unix_millis(now.unix_millis() + 86_400_000)?,
        ),
    );
    let start = PreciseInstant::from_millis(UtcInstant::from_unix_millis(0)?);
    let end = PreciseInstant::from_millis(UtcInstant::from_unix_millis(86_400_000)?);
    let bucket = EvidenceInterval::new(start, end)?;
    let observation = CanonicalObservation::new(
        ObservationContext::new(
            r.scope.instrument,
            capability,
            TransformationRevision::new(1)?,
            day,
            &[],
        )?,
        ObservationVersion::new(ObservationId::new(1)?, ObservationRevision::new(1)?, None)?,
        EvidenceTimes::new(start, None, None, PreciseInstant::from_millis(now))?,
        None,
        Some(ObservationValue::Bar(RawBar::new(
            bucket,
            BarCoverage::ProviderDay,
            [UsdPrice::new(price, 0)?; 4],
            ShareVolume::new(0, 0)?,
        )?)),
    )?;
    let batch = ProviderBatch {
        request: r.id,
        provider: capability,
        observations: vec![observation],
    };
    let buckets = [(day, bucket)];
    let series = DailySeriesSpec {
        instrument: r.scope.instrument,
        listing: ListingId::new(1)?,
        symbol: &symbol,
        venue: Venue::Xnys,
        range: r.range,
        cutoff: now,
        provider: capability,
        buckets: &buckets,
    };
    Ok(TerminalProposal::ready(
        record,
        admitted,
        catalogs,
        &Cancellation::default(),
        EvidenceFreeze {
            command: record.command(),
            batch: &batch,
            series: &series,
            references: DailyReferences {
                mappings: &mappings,
                calendar: &calendar,
                actions: &actions,
                action_coverage: &coverage,
            },
            policy: DailyQualityPolicy::new(TransformationRevision::new(1)?, 60000, 1000, 100)?,
            previous,
        },
        storage,
        access,
    )
    .await?)
}
