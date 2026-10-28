//! Companion namespace: same licensed canonical authority, distinct immutable bytes.
use super::*;
use edgeagent_contracts::evidence::{
    MAX_STREAM_SEGMENT_BYTES, StreamCompanion, StreamCompanionObject,
};
use edgeagent_market_data_application::StreamSegmentAccess;

pub(super) fn key(o: &StreamCompanionObject) -> Path {
    let s = &o.segment.scope;
    Path::from(format!(
        "evidence/stream-companion-v1/{}/{}/{}/{}/{}/{}.json",
        s.storage.as_str(),
        s.audience.as_str(),
        s.rights.id.as_str(),
        s.rights.revision.as_str(),
        s.session.as_str(),
        o.sha256.as_str()
    ))
}
fn guard(o: &StreamCompanionObject, a: &impl StreamSegmentAccess) -> Result<(), Error> {
    if o.bytes == 0 || o.bytes as usize > MAX_STREAM_SEGMENT_BYTES {
        return Err(Error::new(Kind::Invalid));
    }
    super::segment::guard(&o.segment, a)
}
impl ImmutableEvidenceStore {
    pub(super) async fn create_companion(
        &self,
        c: &StreamCompanion,
        a: &impl StreamSegmentAccess,
    ) -> Result<StreamCompanionObject, Error> {
        let object = c.object();
        guard(&object, a)?;
        tokio::time::timeout(self.timeout, async {
            match self
                .backend
                .put_opts(
                    &key(&object),
                    c.bytes().to_vec().into(),
                    PutMode::Create.into(),
                )
                .await
            {
                Ok(_) | Err(object_store::Error::AlreadyExists { .. }) => (),
                Err(e) => return Err(backend_error(e)),
            }
            let actual = self.read_companion(&object, a).await?;
            if actual.bytes() != c.bytes() {
                return Err(Error::new(Kind::Integrity));
            }
            Ok(object)
        })
        .await
        .map_err(|e| Error::with_source(Kind::Deadline, e))?
    }
    pub(super) async fn read_companion(
        &self,
        o: &StreamCompanionObject,
        a: &impl StreamSegmentAccess,
    ) -> Result<StreamCompanion, Error> {
        guard(o, a)?;
        let result = self.backend.get(&key(o)).await.map_err(backend_error)?;
        if result.meta.size != u64::from(o.bytes) || result.range != (0..u64::from(o.bytes)) {
            return Err(Error::new(Kind::Integrity));
        }
        let mut stream = result.into_stream();
        let mut bytes = Vec::with_capacity(o.bytes as usize);
        while let Some(chunk) = stream.try_next().await.map_err(backend_error)? {
            tokio::task::consume_budget().await;
            guard(o, a)?;
            if chunk.len() > o.bytes as usize - bytes.len() {
                return Err(Error::new(Kind::Integrity));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() != o.bytes as usize {
            return Err(Error::new(Kind::Integrity));
        }
        let c =
            StreamCompanion::decode(&bytes, &o.sha256).map_err(|_| Error::new(Kind::Integrity))?;
        if c.object() != *o {
            return Err(Error::new(Kind::Integrity));
        }
        guard(o, a)?;
        Ok(c)
    }
}
