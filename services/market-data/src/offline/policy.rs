use super::*;
pub(super) struct Access<'a> {
    pub job: &'a Job,
    /// One capture instant keeps the series cutoff and artifact freeze identical.
    /// Authorization independently samples wall time before/after storage/publication.
    pub captured: UtcInstant,
}
impl EvidenceAccess for Access<'_> {
    fn now(&self) -> UtcInstant {
        self.captured
    }
    fn authorize(&self, object: &EvidenceObject) -> std::result::Result<(), EvidenceStorageError> {
        let denied = || EvidenceStorageError::new(EvidenceStorageErrorKind::Denied);
        let sample = now().map_err(|_| denied())?;
        let cancellation = Cancellation::default();
        let authority = self
            .job
            .catalogs()
            .admit(self.job.record.request(), sample, &cancellation)
            .map_err(|_| denied())?;
        self.job
            .record
            .verify_authority(&authority)
            .map_err(|_| denied())?;
        let scope = self.job.record.retention_scope().map_err(|_| denied())?;
        let delete = i128::from(object.delete_by.seconds) * 1000
            + i128::from(object.delete_by.nanos / 1_000_000);
        if object.storage != scope.storage
            || object.audience != scope.audience
            || object.rights.id != scope.rights
            || object.rights.revision.as_str() != "1"
            || delete > i128::from(self.job.record.delete_by_ms().map_err(|_| denied())?)
        {
            return Err(denied());
        }
        Ok(())
    }
}
impl PublicationAuthority for Access<'_> {
    fn authorize_publication(
        &self,
        record: &AdmissionRecord,
        object: &EvidenceObject,
    ) -> std::result::Result<(), BudgetError> {
        if record != &self.job.record {
            return Err(BudgetError::new(BudgetErrorKind::Denied));
        }
        self.authorize(object)
            .map_err(|cause| BudgetError::with_source(BudgetErrorKind::Denied, cause))
    }
}
pub(super) struct Cleanup<'a> {
    pub confirmed: &'a BTreeSet<EvidenceId>,
}
impl CleanupAuthority for Cleanup<'_> {
    fn authorize_deletion(
        &self,
        request: &EvidenceId,
        object: &EvidenceObject,
    ) -> std::result::Result<(), BudgetError> {
        self.confirm_writes_quiescent(request)?;
        if &object.storage != request
            || object.audience.as_str() != "00000000000000000000000000000001"
            || object.rights.id.as_str() != "00000000000000000000000000000001"
            || object.rights.revision.as_str() != "1"
        {
            return Err(BudgetError::new(BudgetErrorKind::Denied));
        }
        Ok(())
    }
    fn confirm_writes_quiescent(
        &self,
        request: &EvidenceId,
    ) -> std::result::Result<(), BudgetError> {
        if self.confirmed.contains(request) {
            Ok(())
        } else {
            Err(BudgetError::new(BudgetErrorKind::Denied))
        }
    }
    fn confirm_account_quiescent(
        &self,
        _: &EvidenceId,
        _: i64,
    ) -> std::result::Result<(), BudgetError> {
        // No automatic or synthetic proof can clear uncertainty from a previous worker.
        Err(BudgetError::new(BudgetErrorKind::Denied))
    }
}
