//! Confirmed-state model; PostgreSQL tests separately prove real transactions.
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
pub struct NativeStore {
    pub objects: Mutex<Vec<NativeDailyArtifact>>,
    pub calls: AtomicUsize,
    pub fail: AtomicBool,
}
impl NativeEvidenceStorage for NativeStore {
    async fn put_native(
        &self,
        a: &NativeDailyArtifact,
        access: &impl EvidenceAccess,
    ) -> Result<EvidenceObject, EvidenceStorageError> {
        let object = a
            .object()
            .map_err(|_| EvidenceStorageError::new(EvidenceStorageErrorKind::Invalid))?;
        access.authorize(&object)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.objects
            .lock()
            .map_err(|_| EvidenceStorageError::new(EvidenceStorageErrorKind::Backend))?
            .push(a.clone());
        if self.fail.load(Ordering::SeqCst) {
            return Err(EvidenceStorageError::new(EvidenceStorageErrorKind::Backend));
        }
        Ok(object)
    }
    async fn get_native(
        &self,
        o: &EvidenceObject,
        access: &impl EvidenceAccess,
    ) -> Result<NativeDailyArtifact, EvidenceStorageError> {
        access.authorize(o)?;
        self.objects
            .lock()
            .map_err(|_| EvidenceStorageError::new(EvidenceStorageErrorKind::Backend))?
            .iter()
            .find(|a| a.digest() == &o.sha256)
            .cloned()
            .ok_or_else(|| EvidenceStorageError::new(EvidenceStorageErrorKind::NotFound))
    }
}

#[derive(Default)]
pub struct NativeInventory {
    pub objects: Mutex<Vec<NativeDailyArtifact>>,
    pub denied: AtomicBool,
}
impl EvidenceRetention for NativeInventory {
    async fn stage(&self, _: &DailyEvidenceArtifact) -> Result<(), BudgetError> {
        Err(BudgetError::new(BudgetErrorKind::Invalid))
    }
    async fn check(&self, o: &EvidenceObject) -> Result<(), BudgetError> {
        let exists = self
            .objects
            .lock()
            .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))?
            .iter()
            .any(|a| a.object().as_ref() == Ok(o));
        if self.denied.load(Ordering::SeqCst) || !exists {
            Err(BudgetError::new(BudgetErrorKind::Denied))
        } else {
            Ok(())
        }
    }
    async fn pending_cleanup(&self, _: u8) -> Result<Vec<EvidenceCleanup>, BudgetError> {
        Ok(vec![])
    }
    async fn complete_cleanup(&self, _: &EvidenceId) -> Result<(), BudgetError> {
        Ok(())
    }
}
impl NativeEvidenceRetention for NativeInventory {
    async fn stage_native(&self, a: &NativeDailyArtifact) -> Result<(), BudgetError> {
        if self.denied.load(Ordering::SeqCst) {
            return Err(BudgetError::new(BudgetErrorKind::Denied));
        }
        let mut objects = self
            .objects
            .lock()
            .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))?;
        if let Some(old) = objects
            .iter()
            .find(|p| p.data().capture.scope.request == a.data().capture.scope.request)
        {
            if old != a {
                return Err(BudgetError::new(BudgetErrorKind::Conflict));
            }
        } else {
            objects.push(a.clone());
        }
        Ok(())
    }
    async fn recover_native(
        &self,
        command: &AcquireEvidence,
        access: &impl EvidenceAccess,
    ) -> Result<Option<NativeDailyArtifact>, BudgetError> {
        if self.denied.load(Ordering::SeqCst) {
            return Err(BudgetError::new(BudgetErrorKind::Denied));
        }
        let found = self
            .objects
            .lock()
            .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))?
            .iter()
            .find(|a| a.data().capture.scope.request == command.request)
            .cloned();
        if let Some(a) = &found {
            access
                .authorize(
                    &a.object()
                        .map_err(|_| BudgetError::new(BudgetErrorKind::Invalid))?,
                )
                .map_err(|e| BudgetError::with_source(BudgetErrorKind::Denied, e))?;
        }
        Ok(found)
    }
}
