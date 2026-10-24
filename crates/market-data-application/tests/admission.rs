#[allow(dead_code)]
mod support;
use edgeagent_contracts::evidence::{DailyEvidenceArtifact, EvidenceId};
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use std::error::Error;
use support::*;
type TestResult = std::result::Result<(), Box<dyn Error>>;
fn limits() -> AcquisitionLimits {
    AcquisitionLimits {
        attempts: 3,
        spacing_ms: 15000,
        starts_per_minute: 4,
    }
}

#[test]
fn configuration_v2_roundtrips_without_reinterpreting_legacy_admissions() -> TestResult {
    let fixture = Fixture::new()?;
    let legacy = record(&fixture)?;
    let digest = DailyEvidenceArtifact::hash(b"selected offline configuration");
    let bound = legacy.clone().with_configuration(&digest)?;
    assert_eq!(legacy.configuration(), None);
    assert_eq!(bound.configuration(), Some(digest.as_str()));
    assert_ne!(
        legacy.command().request_sha256,
        bound.command().request_sha256
    );
    assert_eq!(
        AdmissionRecord::decode(legacy.bytes(), legacy.command())?,
        legacy
    );
    assert_eq!(
        AdmissionRecord::decode(bound.bytes(), bound.command())?,
        bound
    );
    let cancel = Cancellation::default();
    let authority = fixture
        .catalogs()
        .admit(&fixture.request, now()?, &cancel)?;
    bound.verify_authority(&authority)?;
    assert!(bound.clone().with_configuration(&digest).is_err());
    for format in [
        "edgeagent.admission.daily.v1",
        "edgeagent.admission.daily.v3",
    ] {
        let mut fields: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(bound.bytes())?;
        fields.insert("format".into(), format.into());
        let mut bytes = serde_json::to_vec(&fields)?;
        bytes.push(b'\n');
        let mut command = bound.command().clone();
        command.request_sha256 = DailyEvidenceArtifact::hash(&bytes);
        assert!(AdmissionRecord::decode(&bytes, &command).is_err());
    }
    Ok(())
}
fn record(f: &Fixture) -> Result<AdmissionRecord> {
    Ok(AdmissionRecord::new(
        &f.catalogs()
            .admit(&f.request, now()?, &Cancellation::default())?,
        limits(),
        ListingId::new(1)?,
        DailyQualityPolicy::new(TransformationRevision::new(1)?, 60000, 1000, 100)?,
    )?)
}
fn kind<T>(result: std::result::Result<T, BudgetError>, expected: BudgetErrorKind) -> TestResult {
    assert_eq!(
        result.err().ok_or("expected budget error")?.kind(),
        expected
    );
    Ok(())
}

#[test]
fn admission_canonical_digest_binds_complete_inputs_and_current_authority() -> TestResult {
    let f = Fixture::new()?;
    let r = record(&f)?;
    assert_eq!(AdmissionRecord::decode(r.bytes(), r.command())?, r);
    assert_eq!(
        r.command().request_sha256,
        DailyEvidenceArtifact::hash(r.bytes())
    );
    let token = f
        .catalogs()
        .admit(&f.request, now()?, &Cancellation::default())?;
    r.verify_authority(&token)?;
    let mut changed = Fixture::new()?;
    changed.request.budget.calls = nz(11)?;
    let other = record(&changed)?;
    assert_ne!(r.command().request_sha256, other.command().request_sha256);
    let quality = DailyQualityPolicy::new(TransformationRevision::new(2)?, 60000, 1000, 100)?;
    let other = AdmissionRecord::new(&token, limits(), ListingId::new(1)?, quality)?;
    assert_ne!(r.command().request_sha256, other.command().request_sha256);
    kind(
        r.verify(&token, ListingId::new(2)?, quality),
        BudgetErrorKind::Conflict,
    )?;
    let mut limits = limits();
    limits.attempts = 0;
    kind(
        AdmissionRecord::new(&token, limits, ListingId::new(1)?, quality),
        BudgetErrorKind::Invalid,
    )?;
    assert!(!format!("{r:?}").contains(r.command().request_sha256.as_str()));
    Ok(())
}
#[test]
fn admission_decode_rejects_noncanonical_unknown_duplicate_and_tampered_records() -> TestResult {
    let r = record(&Fixture::new()?)?;
    let mut fields: std::collections::BTreeMap<String, String> = serde_json::from_slice(r.bytes())?;
    for (key, value) in [
        ("unknown", "x"),
        ("calls", "01"),
        ("knowledge", "0172800000"),
        ("rights", "bad"),
        ("delete_ms", "0"),
        ("reference_age_ms", "0"),
        ("quality_revision", "0"),
    ] {
        let mut changed = fields.clone();
        changed.insert(key.into(), value.into());
        let mut bytes = serde_json::to_vec(&changed)?;
        bytes.push(b'\n');
        let mut command = r.command().clone();
        command.request_sha256 = DailyEvidenceArtifact::hash(&bytes);
        assert!(AdmissionRecord::decode(&bytes, &command).is_err());
    }
    fields.remove("listing");
    let bytes = serde_json::to_vec(&fields)?;
    let mut command = r.command().clone();
    command.request_sha256 = DailyEvidenceArtifact::hash(&bytes);
    assert!(AdmissionRecord::decode(&bytes, &command).is_err());
    let text = std::str::from_utf8(r.bytes())?;
    for text in [
        format!(" {text}"),
        text.replacen('{', "{\"calls\":\"12\",", 1),
    ] {
        command.request_sha256 = DailyEvidenceArtifact::hash(text.as_bytes());
        assert!(AdmissionRecord::decode(text.as_bytes(), &command).is_err());
    }
    assert!(AdmissionRecord::decode(&vec![b' '; 8193], r.command()).is_err());
    let mut bytes = r.bytes().to_vec();
    bytes[0] = b'!';
    assert!(AdmissionRecord::decode(&bytes, r.command()).is_err());
    Ok(())
}
#[test]
fn attempt_call_page_and_byte_budgets_survive_state_recovery() -> TestResult {
    let f = Fixture::new()?;
    let r = record(&f)?;
    let mut b = AcquisitionBudget::default();
    let mut a = AccountBudget::new(limits())?;
    b.begin_attempt(&r, 1, NOW)?;
    let page = EvidenceId::new(&format!("{:032x}", 1))?;
    for i in 0..4 {
        let p = b.start(&mut a, &r, Some(&page), NOW + i * 15000)?;
        b.complete(
            &mut a,
            &r,
            &p,
            CallCompletion::Observed {
                decoded_bytes: 0,
                retry_after_ms: None,
            },
            NOW + i * 15000,
        )?;
    }
    kind(
        b.start(&mut a, &r, Some(&page), NOW + 60000),
        BudgetErrorKind::Exhausted,
    )?;
    b = AcquisitionBudget::decode(&b.encode()?)?;
    a = AccountBudget::decode(&a.encode()?)?;
    assert_eq!((b.snapshot().attempts, b.snapshot().calls), (1, 4));
    let mut foreign = Fixture::new()?;
    foreign.request.id = AcquisitionId::new(2)?;
    kind(b.verify(&record(&foreign)?), BudgetErrorKind::Invalid)?;
    b.begin_attempt(&r, 1, NOW + 60000)?; // Lost attempt-charge response: same ordinal is idempotent.
    assert_eq!(b.snapshot().attempts, 1);
    kind(
        b.start(&mut a, &r, Some(&page), NOW + 60000),
        BudgetErrorKind::Exhausted,
    )?;
    for attempt in 1..3 {
        b.begin_attempt(&r, u8::try_from(attempt + 1)?, NOW + attempt * 60000)?;
        for i in 0..4 {
            let t = NOW + attempt * 60000 + i * 15000;
            let p = b.start(&mut a, &r, Some(&page), t)?;
            b.complete(
                &mut a,
                &r,
                &p,
                CallCompletion::Observed {
                    decoded_bytes: 0,
                    retry_after_ms: None,
                },
                t,
            )?;
        }
    }
    assert_eq!(b.snapshot().calls, 12);
    kind(
        b.begin_attempt(&r, 4, NOW + 180000),
        BudgetErrorKind::Exhausted,
    )?;
    let mut b = AcquisitionBudget::default();
    let mut a = AccountBudget::new(limits())?;
    b.begin_attempt(&r, 1, NOW)?;
    for i in 0..8 {
        let t = NOW + i * 15000;
        let p = b.start(&mut a, &r, None, t)?;
        b.complete(&mut a, &r, &p, CallCompletion::QuiescedUnknown, t)?;
    }
    assert_eq!(b.snapshot().charged_bytes, 8_388_608);
    kind(
        b.start(&mut a, &r, None, NOW + 120000),
        BudgetErrorKind::Exhausted,
    )?;
    Ok(())
}
#[test]
fn account_limits_spacing_window_and_pending_permits_span_commands() -> TestResult {
    let f = Fixture::new()?;
    let r = record(&f)?;
    let mut other = Fixture::new()?;
    other.request.id = AcquisitionId::new(2)?;
    let r2 = record(&other)?;
    let mut a = AccountBudget::new(limits())?;
    let mut b = AcquisitionBudget::default();
    let mut c = AcquisitionBudget::default();
    b.begin_attempt(&r, 1, NOW)?;
    c.begin_attempt(&r2, 1, NOW)?;
    let p = b.start(&mut a, &r, None, NOW)?;
    kind(c.start(&mut a, &r2, None, NOW), BudgetErrorKind::Busy)?;
    b.complete(
        &mut a,
        &r,
        &p,
        CallCompletion::Observed {
            decoded_bytes: 4,
            retry_after_ms: None,
        },
        NOW,
    )?;
    kind(
        c.start(&mut a, &r2, None, NOW + 14999),
        BudgetErrorKind::RateLimited,
    )?;
    let p = c.start(&mut a, &r2, None, NOW + 15000)?;
    c.complete(
        &mut a,
        &r2,
        &p,
        CallCompletion::Observed {
            decoded_bytes: 4,
            retry_after_ms: None,
        },
        NOW + 15000,
    )?;
    a.tighten(AcquisitionLimits {
        attempts: 3,
        spacing_ms: 15000,
        starts_per_minute: 2,
    })?;
    kind(
        c.start(&mut a, &r2, None, NOW + 30000),
        BudgetErrorKind::RateLimited,
    )?;
    // Oldest start is excluded at the exact rolling-minute boundary.
    let p = c.start(&mut a, &r2, None, NOW + 60000)?;
    c.complete(
        &mut a,
        &r2,
        &p,
        CallCompletion::QuiescedUnknown,
        NOW + 60000,
    )?;
    a.tighten(limits())?;
    kind(
        c.start(&mut a, &r2, None, NOW + 74999),
        BudgetErrorKind::RateLimited,
    )?;
    let fields: serde_json::Value = serde_json::from_slice(&a.encode()?)?;
    assert_eq!(fields["starts_per_minute"], 2);
    Ok(())
}
#[test]
fn cancellation_deadlines_and_uncertain_calls_do_not_refund_or_release() -> TestResult {
    let r = record(&Fixture::new()?)?;
    let mut b = AcquisitionBudget::default();
    let mut a = AccountBudget::new(limits())?;
    b.begin_attempt(&r, 1, NOW)?;
    let p = b.start(&mut a, &r, None, NOW)?;
    let snapshot = b.snapshot();
    b = AcquisitionBudget::decode(&b.encode()?)?;
    a = AccountBudget::decode(&a.encode()?)?;
    assert_eq!(b.snapshot(), snapshot);
    kind(b.begin_attempt(&r, 2, NOW + 15000), BudgetErrorKind::Busy)?;
    b.cancel();
    kind(
        b.start(&mut a, &r, None, NOW + 15000),
        BudgetErrorKind::Cancelled,
    )?;
    assert_eq!(b.snapshot().charged_bytes, 1_048_576);
    assert!(b.snapshot().pending.is_some());
    b.complete(
        &mut a,
        &r,
        &p,
        CallCompletion::QuiescedUnknown,
        NOW + 300000,
    )?;
    assert_eq!(b.snapshot().charged_bytes, 1_048_576);
    let mut b = AcquisitionBudget::default();
    kind(
        b.begin_attempt(&r, 1, NOW + 300000),
        BudgetErrorKind::Deadline,
    )?;
    b.begin_attempt(&r, 1, NOW)?;
    let p = b.start(&mut a, &r, None, NOW + 300000 - 1)?;
    b.complete(
        &mut a,
        &r,
        &p,
        CallCompletion::Observed {
            decoded_bytes: 0,
            retry_after_ms: Some(1),
        },
        NOW + 300000 - 1,
    )?;
    kind(
        b.start(&mut a, &r, None, NOW + 300000 - 1),
        BudgetErrorKind::Exhausted,
    )?;
    Ok(())
}
#[test]
fn completion_replay_is_immutable_and_old_fences_cannot_release_new_work() -> TestResult {
    let r = record(&Fixture::new()?)?;
    let mut b = AcquisitionBudget::default();
    let mut a = AccountBudget::new(limits())?;
    b.begin_attempt(&r, 1, NOW)?;
    let p = b.start(&mut a, &r, None, NOW)?;
    let first = CallCompletion::Observed {
        decoded_bytes: 10,
        retry_after_ms: Some(20000),
    };
    b.complete(&mut a, &r, &p, first, NOW)?;
    kind(
        b.start(&mut a, &r, None, NOW + 15000),
        BudgetErrorKind::RateLimited,
    )?;
    let next = b.start(&mut a, &r, None, NOW + 20000)?;
    let before = b.snapshot();
    b.complete(&mut a, &r, &p, first, NOW + 21000)?;
    assert_eq!(b.snapshot(), before);
    kind(
        b.complete(&mut a, &r, &p, CallCompletion::QuiescedUnknown, NOW + 21000),
        BudgetErrorKind::Conflict,
    )?;
    kind(
        b.complete(
            &mut a,
            &r,
            &next,
            CallCompletion::Observed {
                decoded_bytes: 1_048_577,
                retry_after_ms: None,
            },
            NOW + 21000,
        ),
        BudgetErrorKind::Invalid,
    )?;
    assert_eq!(b.snapshot(), before);
    let mut corrupt: serde_json::Value = serde_json::from_slice(&b.encode()?)?;
    let mut missing = corrupt.clone();
    missing.as_object_mut().ok_or("object")?.remove("pending");
    assert!(AcquisitionBudget::decode(&serde_json::to_vec(&missing)?).is_err());
    let mut missing_delay = corrupt.clone();
    missing_delay
        .pointer_mut("/completed/0/outcome/Observed")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("observed receipt")?
        .remove("retry_after_ms");
    assert!(AcquisitionBudget::decode(&serde_json::to_vec(&missing_delay)?).is_err());
    let mut future = corrupt.clone();
    future["version"] = 2.into();
    assert!(AcquisitionBudget::decode(&serde_json::to_vec(&future)?).is_err());
    corrupt["bytes"] = 0.into();
    assert!(AcquisitionBudget::decode(&serde_json::to_vec(&corrupt)?).is_err());
    let error = BudgetError::storage(std::io::Error::other("private-source-sentinel"));
    assert!(!format!("{error} {error:?}").contains("private-source-sentinel"));
    assert!(error.source().is_some());
    Ok(())
}
