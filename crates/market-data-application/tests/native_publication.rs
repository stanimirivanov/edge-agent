#[allow(dead_code)]
mod support;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use support::*;
#[path = "native_acceptance/fixture.rs"]
#[allow(dead_code)]
mod fixture;
#[path = "support/native_store.rs"]
mod model;
#[allow(dead_code)]
#[path = "support/publication.rs"]
mod publication;
use fixture::NativeFixture;
use model::{NativeInventory, NativeStore};
use publication::Access;
use std::sync::atomic::Ordering;

#[test]
fn native_preparation_recovers_original_acceptance_after_uncertain_put() -> Result {
    let f = NativeFixture::new(1)?;
    let objects = NativeStore::default();
    let inventory = NativeInventory::default();
    let storage = RetainedEvidenceStorage::new(&objects, &inventory);
    let access = Access::new(NOW)?;
    let cancel = Cancellation::default();
    let admitted = f.f.catalogs().admit(&f.f.request, now()?, &cancel)?;
    objects.fail.store(true, Ordering::SeqCst);
    let failed = ready(TerminalProposal::ready_native(
        &f.record,
        &admitted,
        f.f.catalogs(),
        &cancel,
        f.input(),
        &storage,
        &access,
    ))?;
    assert!(failed.is_err());
    let retained =
        ready(storage.recover_native(f.record.command(), &access))??.ok_or("preparation")?;
    assert_eq!(retained, f.freeze(f.input())?);
    assert_eq!(objects.calls.load(Ordering::SeqCst), 1);
    objects.fail.store(false, Ordering::SeqCst);
    let later = Access::new(NOW + 1000)?;
    let proposal = ready(TerminalProposal::ready_native(
        &f.record,
        &admitted,
        f.f.catalogs(),
        &cancel,
        f.input(),
        &storage,
        &later,
    ))??;
    assert_eq!(
        proposal.decision(),
        &EvidenceDecision::ready_native(f.record.command().clone(), &retained)?
    );
    assert_eq!(
        proposal.head().ok_or("head")?.captured_at,
        retained.data().captured_at
    );
    assert_eq!(proposal.observations().len(), 2);
    assert_eq!(inventory.objects.lock().map_err(|_| "inventory")?.len(), 1);
    let mut input = f.input();
    input.transformation = TransformationRevision::new(9)?;
    let failure = ready(TerminalProposal::ready_native(
        &f.record,
        &admitted,
        f.f.catalogs(),
        &cancel,
        input,
        &storage,
        &later,
    ))?;
    assert_eq!(
        failure.err().ok_or("conflict")?.kind(),
        BudgetErrorKind::Conflict
    );
    Ok(())
}

#[test]
fn native_publication_requires_authorized_predecessor_and_current_durable_denial() -> Result {
    let first = NativeFixture::new(1)?;
    let previous = first.freeze(first.input())?;
    let next = NativeFixture::new(2)?;
    let objects = NativeStore::default();
    let inventory = NativeInventory::default();
    let storage = RetainedEvidenceStorage::new(&objects, &inventory);
    let access = Access::new(NOW)?;
    let cancel = Cancellation::default();
    let admitted = next.f.catalogs().admit(&next.f.request, now()?, &cancel)?;
    let mut input = next.input();
    input.previous = Some(&previous);
    assert!(
        ready(TerminalProposal::ready_native(
            &next.record,
            &admitted,
            next.f.catalogs(),
            &cancel,
            input,
            &storage,
            &access
        ))?
        .is_err()
    );
    ready(storage.put_native(&previous, &access))??;
    let mut input = next.input();
    input.previous = Some(&previous);
    let proposal = ready(TerminalProposal::ready_native(
        &next.record,
        &admitted,
        next.f.catalogs(),
        &cancel,
        input,
        &storage,
        &access,
    ))??;
    assert_eq!(
        proposal.head().ok_or("head")?.replaces.as_ref(),
        Some(previous.digest())
    );
    inventory.denied.store(true, Ordering::SeqCst);
    assert!(ready(storage.get_native(&previous.object()?, &access))?.is_err());
    assert!(ready(storage.recover_native(next.record.command(), &access))?.is_err());
    assert!(ready(storage.put_native(&previous, &access))?.is_err());
    let counts = objects.calls.load(Ordering::SeqCst);
    access.denied.store(true, Ordering::SeqCst);
    assert!(
        ready(TerminalProposal::ready_native(
            &next.record,
            &admitted,
            next.f.catalogs(),
            &cancel,
            next.input(),
            &storage,
            &access
        ))?
        .is_err()
    );
    assert_eq!(objects.calls.load(Ordering::SeqCst), counts);
    Ok(())
}
