//! Separate stream namespace and typed decoder; daily I/O/retirement is unchanged.
use super::*;
use edgeagent_contracts::evidence::{MAX_STREAM_SEGMENT_BYTES, StreamSegment, StreamSegmentObject};
use edgeagent_market_data_application::{StreamCleanup, StreamDeletion, StreamDeletionAuthority};
use edgeagent_market_data_application::{StreamSegmentAccess, StreamSegmentStorage};

impl StreamDeletion for ImmutableEvidenceStore {
    async fn delete_segment(
        &self,
        cleanup: &StreamCleanup,
        authority: &impl StreamDeletionAuthority,
    ) -> Result<(), Error> {
        authority.confirm_storage_quiescent(cleanup)?;
        tokio::time::timeout(self.timeout, async {
            match self.backend.delete(&key(&cleanup.object)).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
                Err(e) => Err(backend_error(e)),
            }
        })
        .await
        .map_err(|e| Error::with_source(Kind::Deadline, e))??;
        authority.confirm_storage_quiescent(cleanup)
    }
}

impl StreamSegmentStorage for ImmutableEvidenceStore {
    async fn put_segment(
        &self,
        segment: &StreamSegment,
        access: &impl StreamSegmentAccess,
    ) -> Result<StreamSegmentObject, Error> {
        let object = segment.object();
        guard(&object, access)?;
        tokio::time::timeout(self.timeout, async {
            match self
                .backend
                .put_opts(
                    &key(&object),
                    segment.bytes().to_vec().into(),
                    PutMode::Create.into(),
                )
                .await
            {
                Ok(_) | Err(object_store::Error::AlreadyExists { .. }) => (),
                Err(e) => return Err(backend_error(e)),
            }
            let stored = self.read_segment(&object, access).await?;
            if stored.bytes() != segment.bytes() {
                return Err(Error::new(Kind::Integrity));
            }
            Ok(object)
        })
        .await
        .map_err(|e| Error::with_source(Kind::Deadline, e))?
    }
    async fn get_segment(
        &self,
        object: &StreamSegmentObject,
        access: &impl StreamSegmentAccess,
    ) -> Result<StreamSegment, Error> {
        tokio::time::timeout(self.timeout, self.read_segment(object, access))
            .await
            .map_err(|e| Error::with_source(Kind::Deadline, e))?
    }
}
impl ImmutableEvidenceStore {
    async fn read_segment(
        &self,
        object: &StreamSegmentObject,
        access: &impl StreamSegmentAccess,
    ) -> Result<StreamSegment, Error> {
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
        let mut stream = result.into_stream();
        let limit = object.bytes as usize;
        let mut bytes = Vec::with_capacity(limit);
        while let Some(chunk) = stream.try_next().await.map_err(backend_error)? {
            tokio::task::consume_budget().await;
            guard(object, access)?;
            if chunk.len() > limit - bytes.len() {
                return Err(Error::new(Kind::Integrity));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() != limit {
            return Err(Error::new(Kind::Integrity));
        }
        let segment = StreamSegment::decode(&bytes, &object.sha256)
            .map_err(|_| Error::new(Kind::Integrity))?;
        if segment.object() != *object {
            return Err(Error::new(Kind::Integrity));
        }
        guard(object, access)?;
        Ok(segment)
    }
}
fn guard(object: &StreamSegmentObject, access: &impl StreamSegmentAccess) -> Result<(), Error> {
    if object.bytes == 0
        || object.bytes as usize > MAX_STREAM_SEGMENT_BYTES
        || !object.delete_by.nanos.is_multiple_of(1_000_000)
        || object.delete_by.nanos >= 1_000_000_000
        || !(-62_135_596_800..=253_402_300_799).contains(&object.delete_by.seconds)
    {
        return Err(Error::new(Kind::Invalid));
    }
    let deadline = i128::from(object.delete_by.seconds) * 1000
        + i128::from(object.delete_by.nanos / 1_000_000);
    if i128::from(access.now().unix_millis()) >= deadline {
        return Err(Error::new(Kind::Expired));
    }
    access.authorize(object)
}
fn key(o: &StreamSegmentObject) -> Path {
    // No hash-only cross-scope deduplication. All path fragments are checked numeric identities.
    let s = &o.scope;
    Path::from(format!(
        "evidence/stream-segment-v1/{}/{}/{}/{}/{}/{}.json",
        s.storage.as_str(),
        s.audience.as_str(),
        s.rights.id.as_str(),
        s.rights.revision.as_str(),
        s.session.as_str(),
        o.sha256.as_str()
    ))
}
