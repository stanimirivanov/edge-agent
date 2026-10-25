//! Invented owner assessments only; these tests cannot qualify any actual provider/account.
#[allow(dead_code)]
mod support;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use support::{DAY, Fixture, NOW, Result, at, now};
#[allow(dead_code)]
#[path = "native_acceptance/fixture.rs"]
mod native;
use native::NativeFixture;
#[path = "activation/fixture.rs"]
mod fixture;
use fixture::{Case, kind, time};
use std::{error::Error, num::NonZeroU64, sync::atomic::Ordering};

#[test]
fn qualification_profiles_are_bounded_canonical_strict_and_payload_safe() -> Result {
    let c = Case::new(QualificationProvider::TwelveDataDaily, NOW)?;
    let p = &c.profile;
    assert_eq!(QualificationProfile::decode(p.bytes(), p.digest())?, *p);
    assert_eq!(QualificationProfile::new(p.data().clone())?, *p);
    let text = std::str::from_utf8(p.bytes())?;
    for bytes in [
        vec![b' '; 8193],
        format!(" {text}").into_bytes(),
        text.replacen(
            '{',
            "{\"version\":\"edgeagent.qualification-profile.v1\",",
            1,
        )
        .into_bytes(),
        text.replacen('{', "{\"unknown\":\"PRIVATE_PROFILE_SENTINEL\",", 1)
            .into_bytes(),
        text.replace(
            "edgeagent.qualification-profile.v1",
            "edgeagent.qualification-profile.v2",
        )
        .into_bytes(),
    ] {
        let e = QualificationProfile::decode(&bytes, &DailyEvidenceArtifact::hash(&bytes))
            .err()
            .ok_or("invalid profile")?;
        assert!(!format!("{e} {e:?}").contains("PRIVATE_PROFILE_SENTINEL"));
    }
    assert!(
        QualificationProfile::decode(p.bytes(), &DailyEvidenceArtifact::hash(b"other")).is_err()
    );
    for field in [
        "owner_review",
        "credits",
        "admission",
        "configuration_sha256",
        "expires_at",
        "not_before",
    ] {
        let mut data = serde_json::to_value(p.data())?;
        data.as_object_mut().ok_or("object")?.remove(field);
        let bytes = serde_json::to_vec(&data)?;
        assert!(
            QualificationProfile::decode(&bytes, &DailyEvidenceArtifact::hash(&bytes)).is_err()
        );
    }
    for (start, end) in [
        (time(NOW)?, time(NOW)?),
        (time(NOW)?, time(NOW + 300001)?),
        (
            EvidenceTime {
                seconds: 0,
                nanos: 1,
            },
            time(NOW)?,
        ),
        (
            time(NOW)?,
            EvidenceTime {
                seconds: 1,
                nanos: 1_000_000_000,
            },
        ),
    ] {
        let mut data = p.data().clone();
        data.not_before = start;
        data.expires_at = end;
        assert!(QualificationProfile::new(data).is_err());
    }
    assert!(!format!("{p:?} {:?} {:?}", p.data(), c.assessment).contains(p.digest().as_str()));
    Ok(())
}

#[test]
fn qualification_preflight_covers_both_profiles_without_enabling_or_mutating_execution() -> Result {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<QualificationProfile>();
    send_sync::<QualificationPreflight>();
    for provider in [
        QualificationProvider::TwelveDataDaily,
        QualificationProvider::AlpacaHistoricalSipDaily,
    ] {
        let c = Case::new(provider, NOW)?;
        let owner = c.owner();
        let before = c.account.encode()?;
        let report = c.check(&owner, &Cancellation::default())?;
        assert_eq!(report.status(), "preflight_passed_execution_disabled");
        assert_eq!(report.provider(), provider);
        assert_eq!(report.checked_at(), c.now);
        assert_eq!(
            report.valid_until().unix_millis(),
            c.inputs.record.valid_until_ms()?
        );
        assert_eq!(c.account.encode()?, before);
        assert_eq!(
            owner.assessment.as_ref().ok_or("assessment")?.budget,
            c.assessment.budget
        );
        assert_eq!(owner.calls.load(Ordering::SeqCst), 1);
        assert!(!format!("{report:?}").contains(c.profile.digest().as_str()));
    }
    Ok(())
}

#[test]
fn qualification_preflight_requires_independent_current_owner_opt_in() -> Result {
    let c = Case::new(QualificationProvider::TwelveDataDaily, NOW)?;
    let mut owner = c.owner();
    owner.assessment = None;
    kind(
        c.check(&owner, &Cancellation::default()),
        BudgetErrorKind::Denied,
    )?;
    for mutation in 0..7 {
        let mut owner = c.owner();
        let a = owner.assessment.as_mut().ok_or("assessment")?;
        match mutation {
            0 => a.profile_sha256 = DailyEvidenceArtifact::hash(b"unapproved configuration"),
            1 => a.owner_review.revision = EvidenceRevision::new("2")?,
            2 => a.account = ProviderAccountId::new(2)?,
            3 => a.checked_at = at(NOW - 1)?,
            4 => a.checked_at = at(NOW + 1)?,
            5 => a.expires_at = at(c.inputs.record.delete_by_ms()? - 1)?,
            _ => {
                a.credits =
                    DailyCreditPolicy::new(2, 100, NOW + 2 * DAY, &[(ReferenceId::new(1)?, 1)])?
            }
        }
        assert!(
            c.check(&owner, &Cancellation::default()).is_err(),
            "mutation {mutation}"
        );
    }
    owner.failed = Some(BudgetErrorKind::Denied);
    let error = c
        .check(&owner, &Cancellation::default())
        .err()
        .ok_or("owner withdrawal")?;
    assert_eq!(error.kind(), BudgetErrorKind::Denied);
    assert!(error.source().is_some());
    assert!(!format!("{error} {error:?}").contains("OWNER_PRIVATE_SENTINEL"));
    Ok(())
}

#[test]
fn qualification_preflight_rejects_changed_bindings_semantics_authority_and_cancellation() -> Result
{
    let mut c = Case::new(QualificationProvider::TwelveDataDaily, NOW)?;
    let owner = c.owner();
    let original = c.profile.clone();
    for mutation in 0..6 {
        let mut d = original.data().clone();
        match mutation {
            0 => d.admission.request = EvidenceId::new(&format!("{:032x}", 2))?,
            1 => d.admission.request_sha256 = DailyEvidenceArtifact::hash(b"other request"),
            2 => d.configuration_sha256 = DailyEvidenceArtifact::hash(b"other schedule"),
            3 => d.provider = QualificationProvider::AlpacaHistoricalSipDaily,
            4 => d.not_before = time(NOW + 1)?,
            _ => {
                d.not_before = time(NOW + 1)?;
                d.expires_at = time(NOW + 300001)?;
            }
        }
        c.profile = QualificationProfile::new(d)?;
        assert!(c.check(&owner, &Cancellation::default()).is_err());
    }
    c.profile = original;
    assert_eq!(
        owner.calls.load(Ordering::SeqCst),
        0,
        "invalid inputs must not reach owner assessment"
    );
    let cancel = Cancellation::default();
    cancel.cancel();
    kind(c.check(&owner, &cancel), BudgetErrorKind::Cancelled)?;
    let cancel = Cancellation::default();
    let mut late = c.owner();
    late.cancel = Some(cancel.clone());
    kind(c.check(&late, &cancel), BudgetErrorKind::Cancelled)?;
    let mut withdrawn = c.inputs.f.right;
    withdrawn.status = ReviewStatus::Revoked;
    let rights = ReferenceHistory::new(vec![support::version(1, 2, c.now, withdrawn)?])?;
    kind(
        preflight_qualification(
            &c.profile,
            &c.inputs.record,
            AuthorityCatalogs {
                rights: &rights,
                providers: &c.inputs.f.providers,
            },
            c.now,
            &Cancellation::default(),
            &owner,
        ),
        BudgetErrorKind::Denied,
    )?;
    kind(
        preflight_qualification(
            &c.profile,
            &c.inputs.record,
            c.inputs.f.catalogs(),
            at(NOW + 300000)?,
            &Cancellation::default(),
            &owner,
        ),
        BudgetErrorKind::Deadline,
    )?;
    Ok(())
}

#[test]
fn qualification_preflight_requires_every_current_credential_and_lifecycle_control() -> Result {
    let c = Case::new(QualificationProvider::TwelveDataDaily, NOW)?;
    for index in 0..5 {
        for mutation in 0..4 {
            let mut owner = c.owner();
            let a = owner.assessment.as_mut().ok_or("assessment")?;
            match mutation {
                0 => {
                    a.controls.remove(index);
                }
                1 => a.controls[index].known_at = at(NOW + 1)?,
                2 => a.controls[index].expires_at = at(c.inputs.record.delete_by_ms()? - 1)?,
                _ => a.controls[index].control = a.controls[(index + 1) % 5].control,
            }
            kind(
                c.check(&owner, &Cancellation::default()),
                BudgetErrorKind::Denied,
            )?;
        }
    }
    let mut owner = c.owner();
    let a = owner.assessment.as_mut().ok_or("assessment")?;
    a.controls.push(a.controls[0].clone());
    kind(
        c.check(&owner, &Cancellation::default()),
        BudgetErrorKind::Denied,
    )?;
    let mut owner = c.owner();
    owner
        .assessment
        .as_mut()
        .ok_or("assessment")?
        .controls
        .reverse();
    assert!(c.check(&owner, &Cancellation::default()).is_ok());
    Ok(())
}

#[test]
fn qualification_preflight_preserves_uncertainty_and_never_resets_or_guesses_credits() -> Result {
    let c = Case::new(QualificationProvider::TwelveDataDaily, NOW)?;
    for state in [
        QualificationAccountState::Unconfirmed,
        QualificationAccountState::Pending,
        QualificationAccountState::Blocked(NonZeroU64::new(7).ok_or("generation")?),
    ] {
        let mut owner = c.owner();
        owner.assessment.as_mut().ok_or("assessment")?.account_state = state;
        kind(
            c.check(&owner, &Cancellation::default()),
            BudgetErrorKind::Busy,
        )?;
    }
    for mutation in 0..9 {
        let mut owner = c.owner();
        let a = owner.assessment.as_mut().ok_or("assessment")?;
        match mutation {
            0 => a.budget.cancelled = true,
            1 => a.budget.calls = 12,
            2 => a.budget.charged_bytes = c.inputs.record.request().budget.command_bytes.get(),
            3 => a.usage.charged = 89,
            4 => a.usage.revision += 1,
            5 => a.usage.allocation += 1,
            6 => a.usage.expires_ms -= 1,
            7 => a.usage.utc_day -= 1,
            _ => a.usage.utc_day += 1,
        }
        assert!(c.check(&owner, &Cancellation::default()).is_err());
    }
    let mut owner = c.owner();
    owner.assessment.as_mut().ok_or("assessment")?.usage.charged = 88;
    assert!(
        c.check(&owner, &Cancellation::default()).is_ok(),
        "exact full-run credit boundary"
    );
    let mut account = c.account.clone();
    let mut command = AcquisitionBudget::default();
    command.begin_attempt(&c.inputs.record, 1, NOW)?;
    command.start(&mut account, &c.inputs.record, None, NOW)?;
    let before = account.encode()?;
    let mut owner = c.owner();
    owner.assessment.as_mut().ok_or("assessment")?.budget = command.snapshot();
    kind(
        c.check(&owner, &Cancellation::default()),
        BudgetErrorKind::Busy,
    )?;
    assert_eq!(account.encode()?, before);
    let crossing = Case::new(QualificationProvider::TwelveDataDaily, 3 * DAY - 1000)?;
    kind(
        crossing.check(&crossing.owner(), &Cancellation::default()),
        BudgetErrorKind::Exhausted,
    )?;
    Ok(())
}
