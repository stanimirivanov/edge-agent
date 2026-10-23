#[allow(dead_code)]
#[path = "support/publication.rs"]
mod fixtures;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use fixtures::{Access, Result, Storage};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

fn artifact() -> Result<DailyEvidenceArtifact> {
    Ok(DailyEvidenceArtifact::new(serde_json::from_str(
        include_str!("../../contracts/fixtures/evidence-v1/daily.json"),
    )?)?)
}
fn descriptor(a: &DailyEvidenceArtifact) -> Result<EvidenceObject> {
    let d = EvidenceDecision::ready(
        AcquireEvidence {
            request: a.data().scope.request.clone(),
            request_sha256: a.data().scope.request_sha256.clone(),
        },
        a,
    )?;
    match d.data().outcome.clone() {
        EvidenceOutcome::Ready { object } => Ok(object),
        _ => Err("expected ready".into()),
    }
}
struct Inventory {
    denied: AtomicBool,
    pending: Mutex<Vec<EvidenceCleanup>>,
    completed: AtomicUsize,
}
impl Inventory {
    fn new() -> Self {
        Self {
            denied: AtomicBool::new(false),
            pending: Mutex::new(Vec::new()),
            completed: AtomicUsize::new(0),
        }
    }
    fn current(&self) -> std::result::Result<(), BudgetError> {
        if self.denied.load(Ordering::SeqCst) {
            Err(BudgetError::new(BudgetErrorKind::Denied))
        } else {
            Ok(())
        }
    }
}
impl EvidenceRetention for Inventory {
    async fn stage(&self, _: &DailyEvidenceArtifact) -> std::result::Result<(), BudgetError> {
        self.current()
    }
    async fn check(&self, _: &EvidenceObject) -> std::result::Result<(), BudgetError> {
        self.current()
    }
    async fn pending_cleanup(
        &self,
        _: u8,
    ) -> std::result::Result<Vec<EvidenceCleanup>, BudgetError> {
        Ok(self
            .pending
            .lock()
            .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))?
            .clone())
    }
    async fn complete_cleanup(&self, request: &EvidenceId) -> std::result::Result<(), BudgetError> {
        self.pending
            .lock()
            .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))?
            .retain(|i| &i.request != request);
        self.completed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
struct RacingStorage<'a> {
    inner: Storage,
    inventory: &'a Inventory,
    revoke: AtomicBool,
}
impl EvidenceStorage for RacingStorage<'_> {
    async fn put(
        &self,
        a: &DailyEvidenceArtifact,
        access: &impl EvidenceAccess,
    ) -> std::result::Result<EvidenceObject, EvidenceStorageError> {
        let o = self.inner.put(a, access).await?;
        if self.revoke.load(Ordering::SeqCst) {
            self.inventory.denied.store(true, Ordering::SeqCst);
        }
        Ok(o)
    }
    async fn get(
        &self,
        o: &EvidenceObject,
        access: &impl EvidenceAccess,
    ) -> std::result::Result<DailyEvidenceArtifact, EvidenceStorageError> {
        let a = self.inner.get(o, access).await?;
        if self.revoke.load(Ordering::SeqCst) {
            self.inventory.denied.store(true, Ordering::SeqCst);
        }
        Ok(a)
    }
}
fn run(future: impl std::future::Future<Output = Result>) -> Result {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => {
            Err("synthetic proof unexpectedly performed external I/O".into())
        }
    }
}
#[test]
fn durable_denial_checks_surround_storage_and_never_release_revoked_bytes() -> Result {
    run(async {
        let a = artifact()?;
        let o = descriptor(&a)?;
        let inv = Inventory::new();
        let store = RacingStorage {
            inner: Storage::default(),
            inventory: &inv,
            revoke: AtomicBool::new(false),
        };
        let access = Access::new(172_800_000)?;
        let managed = RetainedEvidenceStorage::new(&store, &inv);
        managed.put(&a, &access).await?;
        store.revoke.store(true, Ordering::SeqCst);
        assert_eq!(
            managed
                .get(&o, &access)
                .await
                .err()
                .ok_or("expected denial")?
                .kind(),
            EvidenceStorageErrorKind::Denied
        );
        let calls = store.inner.calls.load(Ordering::SeqCst);
        assert!(managed.put(&a, &access).await.is_err());
        assert_eq!(store.inner.calls.load(Ordering::SeqCst), calls);
        inv.denied.store(false, Ordering::SeqCst);
        assert!(managed.put(&a, &access).await.is_err()); // Revocation after PUT cannot be published.
        assert_eq!(store.inner.objects.lock().map_err(|_| "poison")?.len(), 2); // No compensating deletion.
        Ok(())
    })
}
struct Owner {
    allowed: AtomicBool,
}
impl CleanupAuthority for Owner {
    fn authorize_deletion(
        &self,
        request: &EvidenceId,
        _: &EvidenceObject,
    ) -> std::result::Result<(), BudgetError> {
        self.confirm_writes_quiescent(request)
    }
    fn confirm_writes_quiescent(&self, _: &EvidenceId) -> std::result::Result<(), BudgetError> {
        if self.allowed.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(BudgetError::new(BudgetErrorKind::Denied))
        }
    }
    fn confirm_account_quiescent(
        &self,
        a: &EvidenceId,
        _: i64,
    ) -> std::result::Result<(), BudgetError> {
        self.confirm_writes_quiescent(a)
    }
}
struct Deletes {
    fail: AtomicBool,
    calls: AtomicUsize,
}
impl EvidenceDeletion for Deletes {
    async fn delete(
        &self,
        _: &EvidenceId,
        _: &EvidenceObject,
        _: &impl CleanupAuthority,
    ) -> std::result::Result<(), EvidenceStorageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail.swap(false, Ordering::SeqCst) {
            Err(EvidenceStorageError::new(
                EvidenceStorageErrorKind::Deadline,
            ))
        } else {
            Ok(())
        }
    }
}
#[test]
fn cleanup_requires_quiescence_and_keeps_inventory_after_uncertain_deletion() -> Result {
    run(async {
        let inv = Inventory::new();
        let a = artifact()?;
        let item = EvidenceCleanup {
            request: a.data().scope.request.clone(),
            objects: vec![descriptor(&a)?],
        };
        assert!(
            !format!("{item:?}").contains(item.objects.first().ok_or("object")?.sha256.as_str())
        );
        inv.pending.lock().map_err(|_| "poison")?.push(item);
        let owner = Owner {
            allowed: AtomicBool::new(false),
        };
        let delete = Deletes {
            fail: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
        };
        assert!(cleanup_evidence(&inv, &delete, &owner, 0).await.is_err());
        assert!(cleanup_evidence(&inv, &delete, &owner, 1).await.is_err());
        assert_eq!(delete.calls.load(Ordering::SeqCst), 0);
        owner.allowed.store(true, Ordering::SeqCst);
        assert!(cleanup_evidence(&inv, &delete, &owner, 1).await.is_err());
        assert_eq!(inv.completed.load(Ordering::SeqCst), 0);
        delete.fail.store(false, Ordering::SeqCst);
        assert_eq!(cleanup_evidence(&inv, &delete, &owner, 1).await?, 1);
        assert_eq!(cleanup_evidence(&inv, &delete, &owner, 1).await?, 0);
        let mut other = a.data().clone();
        other.scope.request = EvidenceId::new(&"3".repeat(32))?;
        let other = DailyEvidenceArtifact::new(other)?;
        inv.pending.lock().map_err(|_| "poison")?.extend([
            EvidenceCleanup {
                request: a.data().scope.request.clone(),
                objects: vec![descriptor(&a)?],
            },
            EvidenceCleanup {
                request: other.data().scope.request.clone(),
                objects: vec![descriptor(&other)?],
            },
        ]);
        delete.fail.store(true, Ordering::SeqCst);
        assert!(cleanup_evidence(&inv, &delete, &owner, 2).await.is_err());
        assert_eq!(inv.pending.lock().map_err(|_| "poison")?.len(), 1); // Failure never starves the other request.
        Ok(())
    })
}
