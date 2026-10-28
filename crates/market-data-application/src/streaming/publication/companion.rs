//! Native binding and private object inventory; publication ordering remains in the coordinator.
use super::*;
impl StreamPublication {
    /// Bind complete native provenance before staging; never retrofit a staged intent.
    /// # Errors
    /// Rejects a changed segment, incomplete owner inputs or a deadline beyond approved inputs.
    pub fn with_companion(mut self, companion: StreamCompanion) -> Result<Self> {
        companion.bind(&self.segment).map_err(|_| invalid())?;
        let inputs = StreamOwnerInputs::decode(&companion.data().owner_inputs)?;
        for record in &self.segment.data().records {
            let id =
                u128::from_str_radix(record.observation.id.as_str(), 16).map_err(|_| invalid())?;
            let namespace = (id >> 64) as u64;
            if !inputs
                .claims()
                .iter()
                .any(|claim| claim.namespace() == namespace)
            {
                return Err(invalid());
            }
        }
        if inputs.session_digest() != self.segment.data().scope.session_sha256
            || self.segment.data().delete_by.seconds * 1000
                + i64::from(self.segment.data().delete_by.nanos / 1_000_000)
                > inputs.expires_at().unix_millis()
        {
            return Err(invalid());
        }
        self.companion = Some(companion);
        Ok(self)
    }
    /// Optional complete original native artifact. Absence denotes a legacy canonical-only intent.
    #[must_use]
    pub const fn companion(&self) -> Option<&StreamCompanion> {
        self.companion.as_ref()
    }
    /// Private deletion/read inventory, with legacy descriptor compatibility.
    /// # Errors
    /// Rejects serialization failure.
    pub fn descriptor(&self) -> Result<String> {
        match &self.companion {
            Some(c) => serde_json::to_string(&(
                "edgeagent.stream-publication-objects.v2",
                self.segment.object(),
                c.object(),
            )),
            None => serde_json::to_string(&self.segment.object()),
        }
        .map_err(|_| invalid())
    }
    /// Decode retained object inventory without granting read/deletion authority.
    /// # Errors
    /// Rejects unknown formats or inconsistent pair scope/deadline.
    pub fn objects(bytes: &str) -> Result<(StreamSegmentObject, Option<StreamCompanionObject>)> {
        if bytes.len() > 16384 {
            return Err(invalid());
        }
        if let Ok(single) = serde_json::from_str::<StreamSegmentObject>(bytes) {
            return Ok((single, None));
        }
        let (version, segment, companion): (String, StreamSegmentObject, StreamCompanionObject) =
            serde_json::from_str(bytes).map_err(|_| invalid())?;
        if version != "edgeagent.stream-publication-objects.v2" || companion.segment != segment {
            return Err(invalid());
        }
        Ok((segment, Some(companion)))
    }
}
