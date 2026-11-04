//! Complete owner-selected execution; private constructors seal configuration/reconciliation binding.
use crate::{
    governed::{self, ContextInputs},
    validation,
};
use edgeagent_contracts::{
    research::{ResearchRecord, ResearchRequest},
    research_decision as wire,
    strategy::{Configuration, ExecutionConfiguration, Regime},
    strategy_governance::Profile,
};
use edgeagent_research_domain::{
    EvidencePolicy, FrozenSeries, context_policy as context,
    policy::{self, Intent, Policy},
};

use crate::publication::{PublicationError as Error, PublicationErrorKind as Kind};
fn invalid() -> Error {
    Error::new(Kind::Invalid)
}
/// Existing independent control composition and explicit owner-only lifecycle semantics.
pub const ELIGIBILITY_REVISION: &str = "selected-independent-controls-v1";
/// Legacy explicit-only owner lifecycle.
pub const INVALIDATION_REVISION: &str = "owner-lifecycle-v1";
/// Opt-in retained-decision eligibility invalidation and explicit-cutoff expiry.
pub const CONFIGURED_INVALIDATION_REVISION: &str = "retained-eligibility-expiry-v1";
/// Validated compiled execution selection. No thresholds or allowed regimes are defaulted.
pub struct Selection {
    configuration: ExecutionConfiguration,
    bytes: Vec<u8>,
    digest: String,
    policy: Policy,
    intent: Intent,
}
/// Independently reconciled terminal result bound to the exact selection that actually ran.
pub struct ValidatedExecution {
    pub(crate) outcome: validation::ValidatedOutcome,
    pub(crate) selection: Selection,
}
fn digest(s: &str) -> Result<[u8; 32], Error> {
    if s.len() != 64 {
        return Err(invalid());
    }
    let mut result = [0; 32];
    for (i, b) in result.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
    }
    Ok(result)
}
impl Selection {
    /// Resolve only compiled revisions from a complete canonical selection.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let configuration = ExecutionConfiguration::decode(bytes)
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let ExecutionConfiguration::V1 {
            bundle,
            event_revision,
            context_max_age_ms,
            allowed_regimes,
        } = &configuration;
        let Configuration::V1 {
            strategy: s,
            policy: p,
            ..
        } = bundle;
        if s.id != "momentum"
            || s.score_revision != edgeagent_research_domain::momentum::REVISION
            || s.eligibility_revision != ELIGIBILITY_REVISION
            || s.regime_revision != context::REGIME_REVISION
            || !matches!(
                s.invalidation_revision.as_str(),
                INVALIDATION_REVISION | CONFIGURED_INVALIDATION_REVISION
            )
        {
            return Err(invalid());
        }
        let hash = configuration
            .digest()
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let gate = Policy::new(
            digest(&p.revision)?,
            digest(&hash)?,
            p.instruments
                .iter()
                .map(|v| {
                    edgeagent_research_domain::InstrumentId::new(
                        u128::from_str_radix(v, 16)
                            .map_err(|e| Error::with_source(Kind::Invalid, e))?,
                    )
                    .map_err(|e| Error::with_source(Kind::Invalid, e))
                })
                .collect::<Result<Vec<_>, _>>()?,
            p.max_horizon_sessions,
        )
        .map_err(|e| Error::with_source(Kind::Invalid, e))?
        .with_liquidity(&p.liquidity_revision, p.min_daily_turnover_cents)
        .map_err(|e| Error::with_source(Kind::Invalid, e))?
        .with_market_constraints(
            &p.volatility_revision,
            &p.gap_revision,
            p.max_volatility_ppm,
            p.max_gap_ppm,
        )
        .map_err(|e| Error::with_source(Kind::Invalid, e))?
        .with_context(
            event_revision,
            &s.regime_revision,
            &p.quality_revision,
            context::Rules::new(
                p.event_before_ms,
                p.event_after_ms,
                i64::from(*context_max_age_ms),
                allowed_regimes
                    .iter()
                    .map(|v| match v {
                        Regime::Falling => context::Regime::Falling,
                        Regime::Flat => context::Regime::Flat,
                        Regime::Rising => context::Regime::Rising,
                    })
                    .collect(),
            )
            .map_err(|e| Error::with_source(Kind::Invalid, e))?,
        )
        .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let intent = Intent {
            direction: policy::Direction::Long,
            horizon_sessions: s.horizon_sessions,
        };
        Ok(Self {
            configuration,
            bytes: bytes.to_vec(),
            digest: hash,
            policy: gate,
            intent,
        })
    }
    /// Whether this immutable configuration opts into owner retained-decision checks.
    pub fn configured_invalidation(&self) -> bool {
        let ExecutionConfiguration::V1 { bundle, .. } = &self.configuration;
        let Configuration::V1 { strategy, .. } = bundle;
        strategy.invalidation_revision == CONFIGURED_INVALIDATION_REVISION
    }
    /// Exact retained configuration bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Exact definition requiring separate current owner approval.
    pub fn profile(&self, scope: &str) -> Profile {
        let ExecutionConfiguration::V1 { bundle, .. } = &self.configuration;
        let Configuration::V1 {
            strategy: s,
            policy: p,
            ..
        } = bundle;
        Profile {
            scope: scope.into(),
            implementation: s.implementation.clone(),
            configuration: self.digest.clone(),
            lookback_sessions: s.lookback_sessions,
            max_age_ms: i64::from(p.max_age_ms),
            ttl_ms: i64::from(s.expiry_ms),
            formula: s.score_revision.clone(),
            renderer: crate::RENDERER_REVISION.into(),
        }
    }
    /// Verify immutable provenance references on receipt recovery, without asserting currency.
    pub fn verify_provenance(&self, record: &wire::DecisionProvenance) -> Result<(), Error> {
        let p = self.profile(&record.request.scope);
        let ExecutionConfiguration::V1 { bundle, .. } = &self.configuration;
        let Configuration::V1 { policy, .. } = bundle;
        if record.request.implementation != p.implementation
            || record.request.configuration != p.configuration
            || record.request.lookback_sessions != p.lookback_sessions
            || record.policy != policy.revision
            || record.horizon_sessions != self.intent.horizon_sessions
            || record.direction != wire::Direction::Long
            || record.approval_profile
                != p.digest()
                    .map_err(|e| Error::with_source(Kind::Invalid, e))?
            || record.approval_revision != 2
            || record.validator != validation::REVISION
        {
            return Err(invalid());
        }
        Ok(())
    }
    /// Retain a trusted owner refusal as an abstention, never as positive validation evidence.
    pub fn refused(
        self,
        request: ResearchRequest,
        failure: crate::worker::LoadError,
    ) -> Result<ValidatedExecution, Error> {
        let profile = self.profile(&request.scope);
        if request.configuration != profile.configuration
            || request.implementation != profile.implementation
            || request.lookback_sessions != profile.lookback_sessions
        {
            return Err(invalid());
        }
        use crate::worker::LoadError as E;
        use edgeagent_contracts::research::AbstentionReason as R;
        let reason = match failure {
            E::Unavailable => return Err(Error::with_source(Kind::Storage, failure)),
            E::Denied => R::MissingEntitlement,
            E::Invalid => R::InvalidEvidence,
            E::Unsupported => R::UnsupportedUniverse,
            E::InsufficientHistory => R::InsufficientHistory,
        };
        self.negative(request, reason)
    }
    /// Render a trusted owner evidence abstention; it confers no eligibility decision.
    pub(crate) fn negative(
        self,
        request: ResearchRequest,
        reason: edgeagent_contracts::research::AbstentionReason,
    ) -> Result<ValidatedExecution, Error> {
        let p = self.profile(&request.scope);
        if request.configuration != p.configuration
            || request.implementation != p.implementation
            || request.lookback_sessions != p.lookback_sessions
            || reason == edgeagent_contracts::research::AbstentionReason::NoEligibleCandidate
        {
            return Err(invalid());
        }
        let outcome = validation::refused(request, reason)
            .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        Ok(ValidatedExecution {
            outcome,
            selection: self,
        })
    }
    /// Calculate a proposal and independently reconcile it from the selected immutable inputs.
    pub fn validate(
        self,
        request: ResearchRequest,
        evidence: &EvidencePolicy,
        series: &[FrozenSeries],
        expires_at_ms: i64,
        inputs: ContextInputs<'_>,
    ) -> Result<ValidatedExecution, Error> {
        let p = self.profile(&request.scope);
        if request.implementation != p.implementation
            || request.configuration != p.configuration
            || request.lookback_sessions != p.lookback_sessions
            || evidence.binding().max_age_ms != p.max_age_ms
        {
            return Err(invalid());
        }
        let maximum = request
            .decision_at_ms
            .checked_add(p.ttl_ms)
            .ok_or_else(invalid)?;
        let expected = series
            .iter()
            .map(|v| v.rights_until_ms)
            .min()
            .unwrap_or(maximum)
            .min(maximum);
        if expires_at_ms != expected {
            return Err(invalid());
        }
        let make_inputs = || ContextInputs {
            constraints: inputs.constraints,
            context: inputs.context,
        };
        let proposal = governed::evaluate_with_context_inputs(
            request.clone(),
            evidence,
            series,
            expires_at_ms,
            &self.policy,
            self.intent,
            make_inputs(),
        )
        .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        let outcome = validation::validate(
            proposal.outcome().bytes(),
            validation::Inputs {
                request,
                evidence,
                series,
                expires_at_ms,
                policy: &self.policy,
                intent: self.intent,
                context: make_inputs(),
            },
        )
        .map_err(|e| Error::with_source(Kind::Invalid, e))?;
        Ok(ValidatedExecution {
            outcome,
            selection: self,
        })
    }
}
impl ValidatedExecution {
    /// Independently reconciled terminal record.
    pub fn record(&self) -> &ResearchRecord {
        self.outcome.record()
    }
    pub(crate) fn provenance(&self) -> Result<wire::DecisionProvenance, Error> {
        let request = match self.record() {
            ResearchRecord::Artifact(a) => &a.request,
            ResearchRecord::Abstention(a) => &a.request,
            _ => return Err(invalid()),
        };
        let profile = self.selection.profile(&request.scope);
        let ExecutionConfiguration::V1 { bundle, .. } = &self.selection.configuration;
        let Configuration::V1 { policy: p, .. } = bundle;
        let assessment = if let Some(d) = self.outcome.decision() {
            let status = match d.status() {
                policy::Status::Approved => wire::Status::Approved,
                policy::Status::Narrowed => wire::Status::Narrowed,
                policy::Status::Abstained => wire::Status::Abstained,
            };
            let reasons = d
                .reasons()
                .iter()
                .map(|v| match v {
                    policy::Reason::InstrumentNotAllowed => wire::Reason::InstrumentNotAllowed,
                    policy::Reason::DirectionNotAllowed => wire::Reason::DirectionNotAllowed,
                    policy::Reason::HorizonExceeded => wire::Reason::HorizonExceeded,
                    policy::Reason::LiquidityBelowMinimum => wire::Reason::LiquidityBelowMinimum,
                    policy::Reason::VolatilityExceeded => wire::Reason::VolatilityExceeded,
                    policy::Reason::GapExceeded => wire::Reason::GapExceeded,
                    policy::Reason::EventExcluded => wire::Reason::EventExcluded,
                    policy::Reason::RegimeNotAllowed => wire::Reason::RegimeNotAllowed,
                })
                .collect();
            wire::Assessment::Evaluated {
                status,
                eligible: d
                    .eligible()
                    .iter()
                    .map(|v| format!("{:032x}", v.get()))
                    .collect(),
                reasons,
            }
        } else {
            let ResearchRecord::Abstention(a) = self.record() else {
                return Err(invalid());
            };
            wire::Assessment::NotEvaluated { reason: a.reason }
        };
        Ok(wire::DecisionProvenance {
            request: request.clone(),
            outcome: self.outcome.digest().into(),
            policy: p.revision.clone(),
            direction: wire::Direction::Long,
            horizon_sessions: self.selection.intent.horizon_sessions,
            approval_profile: profile
                .digest()
                .map_err(|e| Error::with_source(Kind::Invalid, e))?,
            approval_revision: 2,
            validator: validation::REVISION.into(),
            assessment,
        })
    }
}
