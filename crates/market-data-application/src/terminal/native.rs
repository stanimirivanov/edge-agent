//! Native acceptance shares terminal COMMIT, but never precise source-time identity lanes.
use super::*;

fn authority_error(error: AcquisitionError) -> Error {
    let kind = match error {
        AcquisitionError::Cancelled => K::Cancelled,
        AcquisitionError::Deadline => K::Deadline,
        _ => K::Denied,
    };
    Error::with_source(kind, error)
}

impl TerminalProposal {
    /// Recover/freeze one native preparation, authenticate predecessor reads and confirm storage.
    /// Call terminal recovery first. On restart call storage's preparation recovery before any
    /// provider work; reconstruct the exact retained inputs, never allocate replacement versions.
    /// # Errors
    /// Denies changed authority, predecessor access, preparation, quality, cancellation or expiry.
    pub async fn ready_native<S: NativeEvidenceStorage, R: NativeEvidenceRetention>(
        record: &AdmissionRecord,
        admitted: &AuthorizedAcquisition<'_>,
        catalogs: AuthorityCatalogs<'_>,
        cancellation: &Cancellation,
        input: NativeEvidenceFreeze<'_>,
        storage: &RetainedEvidenceStorage<'_, S, R>,
        access: &impl PublicationAuthority,
    ) -> Result<Self, Error> {
        let current = catalogs
            .admit(admitted.request(), access.now(), cancellation)
            .map_err(authority_error)?;
        record.verify_authority(&current)?;
        let retained = storage.recover_native(record.command(), access).await?;
        let at = if let Some(a) = &retained {
            let t = a.data().captured_at;
            let millis = i128::from(t.seconds) * 1000 + i128::from(t.nanos / 1_000_000);
            edgeagent_market_data_domain::UtcInstant::from_unix_millis(
                i64::try_from(millis).map_err(|_| Error::new(K::Invalid))?,
            )
            .map_err(|_| Error::new(K::Invalid))?
        } else {
            access.now()
        };
        if let Some(previous) = input.previous {
            let object = previous.object().map_err(|_| Error::new(K::Invalid))?;
            // A supplied digest/DTO is not an authorized predecessor read.
            let loaded = storage
                .get_native(&object, access)
                .await
                .map_err(storage_error)?;
            if loaded != *previous {
                return Err(Error::new(K::Conflict));
            }
        }
        let artifact = freeze_native_evidence(record, admitted, catalogs, at, cancellation, input)
            .map_err(|e| Error::with_source(K::Invalid, e))?;
        if retained.as_ref().is_some_and(|old| old != &artifact) {
            return Err(Error::new(K::Conflict));
        }
        record.verify_native_artifact(&artifact)?;
        let decision = EvidenceDecision::ready_native(record.command().clone(), &artifact)
            .map_err(|_| Error::new(K::Invalid))?;
        let object = artifact.object().map_err(|_| Error::new(K::Invalid))?;
        access.authorize_publication(record, &object)?;
        if storage
            .put_native(&artifact, access)
            .await
            .map_err(storage_error)?
            != object
        {
            return Err(Error::new(K::Invalid));
        }
        catalogs
            .admit(admitted.request(), access.now(), cancellation)
            .map_err(authority_error)?;
        if access.now().unix_millis() >= record.valid_until_ms()? {
            return Err(Error::new(K::Deadline));
        }
        access.authorize(&object).map_err(storage_error)?;
        access.authorize_publication(record, &object)?;
        let data = artifact.data();
        let scope = &data.capture.scope;
        let base = serde_json::to_value((
            &scope.principal,
            &scope.account,
            &scope.source,
            &scope.instrument,
            &scope.audience,
            &scope.storage,
            &scope.endpoint,
            &object.rights.id,
            &data.capture.semantics,
        ))
        .map_err(|_| Error::new(K::Invalid))?;
        let hash = |value| -> Result<EvidenceDigest, Error> {
            Ok(DailyEvidenceArtifact::hash(&canonical(value)?))
        };
        let key = hash(serde_json::json!([
            "edgeagent.native-publication-series.v1",
            base,
            scope.listing,
            data.capture.range
        ]))?;
        let mut observations = Vec::with_capacity(data.lineage.len());
        for (row, lineage) in data.capture.rows.iter().zip(&data.lineage) {
            let identity = hash(serde_json::json!([
                "edgeagent.native-observation.v1",
                base,
                lineage.observation
            ]))?;
            // Full native source provenance/receipt and predecessor are immutable. Selected
            // mapping/actions/transformation are derived metadata and stay outside this guard.
            let fingerprint = hash(serde_json::json!([row, lineage.predecessor]))?;
            observations.push((identity, fingerprint));
        }
        Ok(Self {
            decision,
            head: Some(PublicationHead {
                key,
                digest: artifact.digest().clone(),
                replaces: data.replaces.clone(),
                captured_at: data.captured_at,
                cutoff: data.captured_at,
            }),
            observations,
        })
    }
}
