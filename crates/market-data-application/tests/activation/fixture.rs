use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Owner {
    pub assessment: Option<QualificationAssessment>,
    pub failed: Option<BudgetErrorKind>,
    pub cancel: Option<Cancellation>,
    pub calls: AtomicUsize,
}
impl QualificationAuthority for Owner {
    fn assess(
        &self,
        _: &QualificationProfile,
        _: &AdmissionRecord,
        _: UtcInstant,
    ) -> std::result::Result<Option<QualificationAssessment>, BudgetError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
        if let Some(kind) = self.failed {
            return Err(BudgetError::with_source(
                kind,
                std::io::Error::other("OWNER_PRIVATE_SENTINEL"),
            ));
        }
        Ok(self.assessment.clone())
    }
}
pub fn version(id: u128) -> Result<EvidenceVersion> {
    Ok(EvidenceVersion {
        id: EvidenceId::new(&format!("{id:032x}"))?,
        revision: EvidenceRevision::new("1")?,
    })
}
pub fn time(ms: i64) -> Result<EvidenceTime> {
    Ok(EvidenceTime {
        seconds: ms.div_euclid(1000),
        nanos: u32::try_from(ms.rem_euclid(1000))? * 1_000_000,
    })
}
pub struct Case {
    pub inputs: NativeFixture,
    pub profile: QualificationProfile,
    pub assessment: QualificationAssessment,
    pub account: AccountBudget,
    pub now: UtcInstant,
}
impl Case {
    pub fn new(provider: QualificationProvider, ms: i64) -> Result<Self> {
        let mut inputs = NativeFixture::at(1, ms)?;
        let now = at(ms)?;
        if provider == QualificationProvider::AlpacaHistoricalSipDaily {
            let f = &mut inputs.f;
            f.request.contract = EvidenceContract::new(
                EvidenceKind::RawBar(BarCoverage::ProviderDay),
                MarketScope::Consolidated,
                f.request.contract.rules().0,
                TimestampPrecision::Nanoseconds,
            )?;
            f.right.contract = f.request.contract;
            f.review.capability = ProviderCapability::new(
                f.request.scope.source,
                f.request.contract,
                DeliveryCapability::Snapshots,
                KnowledgeCapability::CaptureOnly,
            );
            f.rights = ReferenceHistory::new(vec![support::version(1, 1, at(0)?, f.right)?])?;
            f.providers = ReferenceHistory::new(vec![support::version(1, 1, at(0)?, f.review)?])?;
            let admitted = f
                .catalogs()
                .admit(&f.request, now, &Cancellation::default())?;
            inputs.record = AdmissionRecord::new(
                &admitted,
                inputs.record.limits(),
                ListingId::new(1)?,
                inputs.policy,
            )?
            .with_configuration(&DailyEvidenceArtifact::hash(
                b"invented-precise-configuration",
            ))?;
        }
        let record = &inputs.record;
        let credits = DailyCreditPolicy::new(
            1,
            100,
            ms + 2 * DAY,
            &[(record.request().scope.endpoint, 1)],
        )?;
        let mut account = AccountBudget::new(record.limits())?;
        account.configure_daily_credits(credits.clone(), 0, ms)?;
        let profile = QualificationProfile::new(QualificationProfileData {
            version: QualificationProfileFormat::V1,
            provider,
            admission: record.command().clone(),
            configuration_sha256: EvidenceDigest::new(
                record.configuration().ok_or("configuration")?,
            )?,
            owner_review: version(900)?,
            credits: credits.clone(),
            not_before: time(ms)?,
            expires_at: time(record.valid_until_ms()?)?,
        })?;
        use QualificationControl::*;
        let controls = [
            DataOnlyCredentialScope,
            ReferenceProvenance,
            RuntimeFencing,
            RetentionAndCleanup,
            RestoreAndConsumers,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, control)| -> Result<_> {
            Ok(QualificationControlReview {
                control,
                evidence: version(70 + u128::try_from(i)?)?,
                known_at: now,
                expires_at: at(record.delete_by_ms()?)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
        let assessment = QualificationAssessment {
            profile_sha256: profile.digest().clone(),
            owner_review: profile.data().owner_review.clone(),
            checked_at: now,
            expires_at: at(record.delete_by_ms()?)?,
            account: record.request().scope.account,
            credits,
            usage: account.daily_usage().ok_or("configured credits")?,
            budget: AcquisitionBudget::default().snapshot(),
            account_state: QualificationAccountState::Quiescent,
            controls,
        };
        Ok(Self {
            inputs,
            profile,
            assessment,
            account,
            now,
        })
    }
    pub fn owner(&self) -> Owner {
        Owner {
            assessment: Some(self.assessment.clone()),
            failed: None,
            cancel: None,
            calls: AtomicUsize::new(0),
        }
    }
    pub fn check(
        &self,
        owner: &Owner,
        cancel: &Cancellation,
    ) -> std::result::Result<QualificationPreflight, BudgetError> {
        preflight_qualification(
            &self.profile,
            &self.inputs.record,
            self.inputs.f.catalogs(),
            self.now,
            cancel,
            owner,
        )
    }
}
pub fn kind<T>(result: std::result::Result<T, BudgetError>, expected: BudgetErrorKind) -> Result {
    assert_eq!(result.err().ok_or("expected failure")?.kind(), expected);
    Ok(())
}
