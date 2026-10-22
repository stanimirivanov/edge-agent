use edgeagent_contracts::evidence::{
    DailyEvidenceArtifact, EvidenceObject, EvidenceReferenceKind, MAX_EVIDENCE_BYTES,
};
use edgeagent_market_data_application::{
    EvidenceAccess, EvidenceStorage, EvidenceStorageError as Error,
    EvidenceStorageErrorKind as Kind,
};
use futures_util::TryStreamExt;
use object_store::{ObjectStore, ObjectStoreExt, PutMode, path::Path};
use std::{fmt, sync::Arc, time::Duration};

#[cfg(test)]
mod tests;

/// SDK adapter with an explicit positive whole-operation timeout, at most 15 seconds.
/// Inject a dedicated, trusted storage namespace; no ambient bucket/credential discovery.
/// Cloud deployments must separately qualify conditional creation, encryption and IAM.
#[derive(Clone)]
pub struct ImmutableEvidenceStore {
    backend: Arc<dyn ObjectStore>,
    timeout: Duration,
}
impl fmt::Debug for ImmutableEvidenceStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImmutableEvidenceStore")
            .finish_non_exhaustive()
    }
}
impl ImmutableEvidenceStore {
    /// Bind a reviewed backend; never falls back to overwrite on unsupported conditional puts.
    /// # Errors
    /// Returns Invalid for zero or greater-than-15-second operation timeout.
    pub fn new(backend: Arc<dyn ObjectStore>, timeout: Duration) -> Result<Self, Error> {
        if timeout.is_zero() || timeout > Duration::from_secs(15) {
            return Err(Error::new(Kind::Invalid));
        }
        Ok(Self { backend, timeout })
    }

    async fn read(
        &self,
        object: &EvidenceObject,
        access: &impl EvidenceAccess,
    ) -> Result<DailyEvidenceArtifact, Error> {
        guard(object, access)?;
        let result = self
            .backend
            .get(&key(object))
            .await
            .map_err(backend_error)?;
        if result.meta.size != u64::from(object.bytes)
            || result.range != (0..u64::from(object.bytes))
        {
            return Err(Error::new(Kind::Integrity));
        }
        collect(result.into_stream(), object, access).await
    }
}
impl EvidenceStorage for ImmutableEvidenceStore {
    async fn put(
        &self,
        artifact: &DailyEvidenceArtifact,
        access: &impl EvidenceAccess,
    ) -> Result<EvidenceObject, Error> {
        let object = descriptor(artifact)?;
        guard(&object, access)?;
        tokio::time::timeout(self.timeout, async {
            match self
                .backend
                .put_opts(
                    &key(&object),
                    artifact.bytes().to_vec().into(),
                    PutMode::Create.into(),
                )
                .await
            {
                Ok(_) | Err(object_store::Error::AlreadyExists { .. }) => (),
                Err(error) => return Err(backend_error(error)),
            }
            // Verify even after a successful PUT: ETags and acknowledgements are not evidence hashes.
            let stored = self.read(&object, access).await?;
            if stored.bytes() != artifact.bytes() {
                return Err(Error::new(Kind::Integrity));
            }
            Ok(object)
        })
        .await
        .map_err(|error| Error::with_source(Kind::Deadline, error))?
    }

    async fn get(
        &self,
        object: &EvidenceObject,
        access: &impl EvidenceAccess,
    ) -> Result<DailyEvidenceArtifact, Error> {
        tokio::time::timeout(self.timeout, self.read(object, access))
            .await
            .map_err(|error| Error::with_source(Kind::Deadline, error))?
    }
}

fn guard(object: &EvidenceObject, access: &impl EvidenceAccess) -> Result<(), Error> {
    if object.bytes == 0
        || object.bytes as usize > MAX_EVIDENCE_BYTES
        || object.delete_by.nanos >= 1_000_000_000
        || !object.delete_by.nanos.is_multiple_of(1_000_000)
        || object.replaces.as_ref() == Some(&object.sha256)
    {
        return Err(Error::new(Kind::Invalid));
    }
    let now = access.now().unix_millis();
    let deadline = object
        .delete_by
        .seconds
        .checked_mul(1000)
        .and_then(|s| s.checked_add(i64::from(object.delete_by.nanos / 1_000_000)))
        .ok_or_else(|| Error::new(Kind::Invalid))?;
    if now >= deadline {
        return Err(Error::new(Kind::Expired));
    }
    access.authorize(object)
}

fn descriptor(artifact: &DailyEvidenceArtifact) -> Result<EvidenceObject, Error> {
    let data = artifact.data();
    let rights = data
        .references
        .iter()
        .find(|r| r.kind == EvidenceReferenceKind::Rights)
        .ok_or_else(|| Error::new(Kind::Invalid))?;
    Ok(EvidenceObject {
        sha256: artifact.digest().clone(),
        bytes: u32::try_from(artifact.bytes().len()).map_err(|_| Error::new(Kind::Invalid))?,
        storage: data.scope.storage.clone(),
        audience: data.scope.audience.clone(),
        rights: rights.version.clone(),
        delete_by: data.delete_by,
        replaces: data.replaces.clone(),
    })
}

fn key(object: &EvidenceObject) -> Path {
    // Every segment is already a checked numeric/hex identity; no URLs or user paths.
    Path::from(format!(
        "evidence/daily-v1/{}/{}/{}/{}/{}.json",
        object.storage.as_str(),
        object.audience.as_str(),
        object.rights.id.as_str(),
        object.rights.revision.as_str(),
        object.sha256.as_str()
    ))
}

async fn collect(
    mut stream: futures_util::stream::BoxStream<'_, object_store::Result<bytes::Bytes>>,
    object: &EvidenceObject,
    access: &impl EvidenceAccess,
) -> Result<DailyEvidenceArtifact, Error> {
    let limit = object.bytes as usize;
    let mut bytes = Vec::with_capacity(limit);
    while let Some(chunk) = stream.try_next().await.map_err(backend_error)? {
        // Ready/empty SDK chunks must not starve the whole-operation timeout.
        tokio::task::consume_budget().await;
        guard(object, access)?;
        // Check before extending, including dishonest content-length/metadata responses.
        if chunk.len() > limit - bytes.len() {
            return Err(Error::new(Kind::Integrity));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() != limit {
        return Err(Error::new(Kind::Integrity));
    }
    let artifact = DailyEvidenceArtifact::decode(&bytes, &object.sha256)
        .map_err(|_| Error::new(Kind::Integrity))?;
    if descriptor(&artifact)? != *object {
        return Err(Error::new(Kind::Integrity));
    }
    guard(object, access)?;
    Ok(artifact)
}

fn backend_error(error: object_store::Error) -> Error {
    let kind = if matches!(error, object_store::Error::NotFound { .. }) {
        Kind::NotFound
    } else {
        Kind::Backend
    };
    Error::with_source(kind, error)
}
