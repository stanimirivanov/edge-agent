use super::*;
/// Immutable owner input supplies catalog content. Durable SQL grant/denial guards
/// remain authoritative on each call, stage, read and terminal commit.
pub(super) struct Access<'a> {
    pub input: &'a input::Input,
    pub record: &'a AdmissionRecord,
    pub captured: UtcInstant,
}
impl DispatchAuthority for Access<'_> {
    fn check(&self, r: &AdmissionRecord, c: &Cancellation) -> Result<DispatchCheck> {
        if r != self.record {
            return Err(denied(()));
        }
        DispatchCheck::new(r, self.input.catalogs(), now()?, c)
    }
}
impl EvidenceAccess for Access<'_> {
    fn now(&self) -> UtcInstant {
        self.captured
    }
    fn authorize(&self, o: &EvidenceObject) -> std::result::Result<(), EvidenceStorageError> {
        let check = || -> Result {
            let at = now()?;
            let authority = self
                .input
                .catalogs()
                .admit(self.record.request(), at, &Cancellation::default())
                .map_err(denied)?;
            self.record.verify_authority(&authority)?;
            let scope = self.record.retention_scope()?;
            let end =
                i128::from(o.delete_by.seconds) * 1000 + i128::from(o.delete_by.nanos / 1_000_000);
            if o.storage != scope.storage
                || o.audience != scope.audience
                || o.rights.id != scope.rights
                || end > i128::from(self.record.delete_by_ms()?)
                || end <= i128::from(at.unix_millis())
            {
                return Err(denied(()));
            }
            Ok(())
        };
        check().map_err(|e| EvidenceStorageError::with_source(EvidenceStorageErrorKind::Denied, e))
    }
}
impl PublicationAuthority for Access<'_> {
    fn authorize_publication(&self, r: &AdmissionRecord, o: &EvidenceObject) -> Result {
        if r != self.record {
            return Err(denied(()));
        }
        self.authorize(o).map_err(denied)
    }
}
