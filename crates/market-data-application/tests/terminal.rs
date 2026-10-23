#[allow(dead_code)]
#[path = "support/publication.rs"]
mod fixtures;
#[allow(dead_code)]
mod support;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use fixtures::{Access, Storage, delivery};
use std::{
    error::Error,
    sync::{
        Mutex,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
};
type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;
#[derive(Default)]
struct Store {
    receipt: Mutex<Option<TerminalReceipt>>,
    mode: AtomicU8,
    commits: AtomicUsize,
}
impl AcquisitionPublication for Store {
    async fn recover(
        &self,
        _: &edgeagent_contracts::MessageEnvelope,
    ) -> std::result::Result<Option<TerminalReceipt>, BudgetError> {
        Ok(self
            .receipt
            .lock()
            .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))?
            .clone())
    }
    async fn commit(
        &self,
        d: &edgeagent_contracts::MessageEnvelope,
        p: &TerminalProposal,
        _: &impl PublicationAuthority,
    ) -> std::result::Result<TerminalReceipt, BudgetError> {
        self.commits.fetch_add(1, Ordering::SeqCst);
        let r = TerminalReceipt::new(d, p.decision(), "2026-10-23T00:00:00Z")?;
        let mode = self.mode.load(Ordering::SeqCst);
        if mode != 1 {
            *self
                .receipt
                .lock()
                .map_err(|_| BudgetError::new(BudgetErrorKind::Storage))? = Some(r.clone());
        }
        if mode == 3 {
            std::future::pending::<()>().await;
        }
        if mode != 0 {
            return Err(BudgetError::new(BudgetErrorKind::Storage));
        }
        Ok(r)
    }
}
fn record(f: &support::Fixture) -> Result<AdmissionRecord> {
    Ok(AdmissionRecord::new(
        &f.catalogs()
            .admit(&f.request, support::now()?, &Cancellation::default())?,
        AcquisitionLimits {
            attempts: 3,
            spacing_ms: 15000,
            starts_per_minute: 4,
        },
        edgeagent_market_data_domain::ListingId::new(1)?,
        edgeagent_market_data_domain::DailyQualityPolicy::new(
            edgeagent_market_data_domain::TransformationRevision::new(1)?,
            60000,
            1000,
            100,
        )?,
    )?)
}
#[test]
fn committed_recovery_skips_preparation_and_preserves_original_event_bytes() -> Result {
    let r = record(&support::Fixture::new()?)?;
    let d = delivery(r.command(), "first")?;
    let store = Store::default();
    let access = Access::new(support::NOW)?;
    let first = support::ready(finish_acquisition(&store, &d, &access, || async {
        Ok(TerminalProposal::rejected(
            r.command().clone(),
            EvidenceRejection::Provider,
        ))
    }))??;
    let next = delivery(r.command(), "logical-retry")?;
    let recovered = support::ready(finish_acquisition(&store, &next, &access, || async {
        Err(BudgetError::new(BudgetErrorKind::Invalid))
    }))??;
    assert_eq!(first.bytes(), recovered.bytes());
    assert_eq!(recovered.envelope().extension("causationid"), Some("first"));
    assert_eq!(store.commits.load(Ordering::SeqCst), 1);
    Ok(())
}
#[test]
fn failed_or_cancelled_confirmation_recovers_without_compensation() -> Result {
    let r = record(&support::Fixture::new()?)?;
    let d = delivery(r.command(), "first")?;
    let access = Access::new(support::NOW)?;
    for mode in [1, 2, 3] {
        let store = Store::default();
        store.mode.store(mode, Ordering::SeqCst);
        let prepare = || async {
            Ok(TerminalProposal::rejected(
                r.command().clone(),
                EvidenceRejection::Provider,
            ))
        };
        if mode == 3 {
            use std::{
                future::Future,
                task::{Context, Poll, Waker},
            };
            let mut attempt = Box::pin(finish_acquisition(&store, &d, &access, prepare));
            assert!(matches!(
                attempt
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            ));
            drop(attempt);
        } else {
            assert!(support::ready(finish_acquisition(&store, &d, &access, prepare))?.is_err());
        }
        store.mode.store(0, Ordering::SeqCst);
        let prepared = AtomicUsize::new(0);
        support::ready(finish_acquisition(&store, &d, &access, || async {
            prepared.fetch_add(1, Ordering::SeqCst);
            Ok(TerminalProposal::rejected(
                r.command().clone(),
                EvidenceRejection::Provider,
            ))
        }))??;
        assert_eq!(prepared.load(Ordering::SeqCst), usize::from(mode == 1));
    }
    Ok(())
}
#[test]
fn ready_preparation_requires_matching_admission_storage_and_current_authority() -> Result {
    let f = support::Fixture::new()?;
    let r = record(&f)?;
    let c = Cancellation::default();
    let admitted = f.catalogs().admit(&f.request, support::now()?, &c)?;
    let access = Access::new(support::NOW)?;
    let storage = Storage::default();
    let ready = support::ready(fixtures::prepare(
        &r,
        &admitted,
        f.catalogs(),
        &storage,
        &access,
        None,
        100,
    ))?
    .map_err(|e| -> Box<dyn Error> { e })?;
    assert!(matches!(
        ready.decision().data().outcome,
        EvidenceOutcome::Ready { .. }
    ));
    assert_eq!(storage.calls.load(Ordering::SeqCst), 1);
    assert!(ready.head().is_some());
    assert_eq!(ready.observations().len(), 1);
    // Independent invented raw-row encoding; protects identity across serde feature unification.
    let expected = concat!(
        r#"{"bucket":[{"nanos":0,"seconds":0},{"nanos":0,"seconds":86400}],"conditions":[],"date":"1970-01-01","#,
        r#""event_at":{"nanos":0,"seconds":0},"observation":{"id":"00000000000000000000000000000001","revision":"1"},"#,
        r#""observed_at":null,"ohlc":[{"coefficient":"100","scale":0},{"coefficient":"100","scale":0},{"coefficient":"100","scale":0},{"coefficient":"100","scale":0}],"#,
        r#""predecessor":null,"published_at":null,"received_at":{"nanos":0,"seconds":172800},"sequence":null,"volume":{"coefficient":"0","scale":0}}"#,
    );
    assert_eq!(
        ready.observations().first().ok_or("fingerprint")?.1,
        DailyEvidenceArtifact::hash(expected.as_bytes())
    );
    let later = Access::new(support::NOW + 1000)?;
    let refreshed = f.catalogs().admit(&f.request, later.now(), &c)?;
    let later_ready = support::ready(fixtures::prepare(
        &r,
        &refreshed,
        f.catalogs(),
        &storage,
        &later,
        None,
        100,
    ))?
    .map_err(|e| -> Box<dyn Error> { e })?;
    let (EvidenceOutcome::Ready { object: first }, EvidenceOutcome::Ready { object: next }) = (
        &ready.decision().data().outcome,
        &later_ready.decision().data().outcome,
    ) else {
        return Err("ready retention".into());
    };
    assert_eq!(first.delete_by, next.delete_by);
    storage.fail.store(true, Ordering::SeqCst);
    assert!(
        support::ready(fixtures::prepare(
            &r,
            &admitted,
            f.catalogs(),
            &storage,
            &access,
            None,
            100
        ))?
        .is_err()
    );
    storage.fail.store(false, Ordering::SeqCst);
    access.denied.store(true, Ordering::SeqCst);
    assert!(
        support::ready(fixtures::prepare(
            &r,
            &admitted,
            f.catalogs(),
            &storage,
            &access,
            None,
            100
        ))?
        .is_err()
    );
    let mut foreign = support::Fixture::new()?;
    foreign.request.budget.calls = std::num::NonZeroU32::new(11).ok_or("calls")?;
    let token = foreign
        .catalogs()
        .admit(&foreign.request, support::now()?, &c)?;
    let before = storage.calls.load(Ordering::SeqCst);
    assert!(
        support::ready(fixtures::prepare(
            &r,
            &token,
            foreign.catalogs(),
            &storage,
            &access,
            None,
            100
        ))?
        .is_err()
    );
    assert_eq!(storage.calls.load(Ordering::SeqCst), before);
    Ok(())
}
#[test]
fn ready_guards_pending_cancelled_and_expired_work_while_rejection_retains_charges() -> Result {
    let f = support::Fixture::new()?;
    let r = record(&f)?;
    let c = Cancellation::default();
    let admitted = f.catalogs().admit(&f.request, support::now()?, &c)?;
    let access = Access::new(support::NOW)?;
    let storage = Storage::default();
    let p = support::ready(fixtures::prepare(
        &r,
        &admitted,
        f.catalogs(),
        &storage,
        &access,
        None,
        100,
    ))?
    .map_err(|e| -> Box<dyn Error> { e })?;
    let mut b = AcquisitionBudget::default();
    let mut a = AccountBudget::new(r.limits())?;
    p.verify(&r, &b.snapshot(), support::NOW)?;
    b.begin_attempt(&r, 1, support::NOW)?;
    b.start(&mut a, &r, None, support::NOW)?;
    assert_eq!(
        p.verify(&r, &b.snapshot(), support::NOW)
            .err()
            .ok_or("pending")?
            .kind(),
        BudgetErrorKind::Busy
    );
    b.cancel();
    assert_eq!(
        p.verify(&r, &b.snapshot(), support::NOW)
            .err()
            .ok_or("cancel")?
            .kind(),
        BudgetErrorKind::Cancelled
    );
    let before = b.snapshot();
    TerminalProposal::rejected(r.command().clone(), EvidenceRejection::Cancelled).verify(
        &r,
        &before,
        support::NOW + 300000,
    )?;
    assert_eq!(b.snapshot(), before);
    assert!(
        p.verify(
            &r,
            &AcquisitionBudget::default().snapshot(),
            support::NOW + 300000
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn receipts_bind_exact_route_command_identity_and_payload_safe_diagnostics() -> Result {
    let r = record(&support::Fixture::new()?)?;
    let d = delivery(r.command(), "command")?;
    let decision = EvidenceDecision::rejected(r.command().clone(), EvidenceRejection::Budget);
    let receipt = TerminalReceipt::new(&d, &decision, "2026-10-23T00:00:00Z")?;
    assert_eq!(
        TerminalReceipt::decode(receipt.bytes(), r.command())?.bytes(),
        receipt.bytes()
    );
    assert!(!format!("{receipt:?}").contains(r.command().request.as_str()));
    let mut wrong = r.command().clone();
    wrong.request_sha256 = DailyEvidenceArtifact::hash(b"different");
    assert!(TerminalReceipt::decode(receipt.bytes(), &wrong).is_err());
    assert!(TerminalReceipt::decode(&vec![b' '; 262145], r.command()).is_err());
    let mut changed = receipt.bytes().to_vec();
    changed.push(b' ');
    assert!(TerminalReceipt::decode(&changed, r.command()).is_err());
    Ok(())
}
