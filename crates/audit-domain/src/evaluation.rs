//! Read-only qualification of bounded synthetic, fully closed account runs.
//! Freeze the complete case census before inspecting outcomes. Content hashes
//! are not registration timestamps, signatures, or evidence of market fidelity.

mod report;
pub use report::{CaseMetrics, EvaluationReport};

use crate::AccountProjection;
use edgeagent_contracts::audit::AccountAuditFrame;
use edgeagent_market_data_domain::PreciseInstant;
use edgeagent_simulation_domain::{ArtifactDigest, Run, RunManifest, execution::ExecutionProfile};
use serde::Serialize;

/// Payload-free evaluation rejection; no partial report is published.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EvaluationError {
    /// Invalid bounds, missing axes, duplicate cases or multi-factor changes.
    Plan,
    /// Selection overlaps the holdout, evidence is future-known or time regresses.
    Chronology,
    /// Run/configuration/plan, funding or population differs from the frozen case.
    Scope,
    /// Missing revisions, unconsumed inputs, open exposure or active orders/holds.
    Incomplete,
    /// Checkpoint, owner chain or exact financial reconciliation failed.
    Fidelity,
    /// Checked integer or bounded encoding capacity exhausted.
    Capacity,
}
impl std::fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frozen evaluation rejected: {self:?}")
    }
}
impl std::error::Error for EvaluationError {}

/// A predeclared one-factor experiment, not a fitted/selected model.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitivityAxis {
    /// Original explicitly supplied profile; exactly one, at index zero.
    Baseline,
    /// Only fixed/per-unit fees may differ.
    Cost,
    /// Only virtual submission/cancellation delays may differ.
    Latency,
    /// Only the whole-unit participation ceiling may differ.
    Liquidity,
}

/// Retained owner configuration and execution/postlude identity for one case.
/// Reconstruct these from trusted composition artifacts, not payload assertions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvaluationCase {
    axis: SensitivityAxis,
    profile: ExecutionProfile,
    configuration: ArtifactDigest,
    execution: ArtifactDigest,
    settlement_policy: Option<ArtifactDigest>,
    settlement: Option<ArtifactDigest>,
    revisions: u64,
}
impl EvaluationCase {
    /// Declare all bindings and the expected final owner revision. Validation
    /// against the baseline and finite bounds occurs in `EvaluationPlan::new`.
    /// Optional settlement is (unchanged policy/evidence digest, owner postlude
    /// plan digest); the latter can vary because it also binds execution.
    #[must_use]
    pub const fn new(
        axis: SensitivityAxis,
        profile: ExecutionProfile,
        configuration: ArtifactDigest,
        execution: ArtifactDigest,
        settlement: Option<(ArtifactDigest, ArtifactDigest)>,
        revisions: u64,
    ) -> Self {
        Self {
            axis,
            profile,
            configuration,
            execution,
            settlement_policy: match settlement {
                Some((policy, _)) => Some(policy),
                None => None,
            },
            settlement: match settlement {
                Some((_, plan)) => Some(plan),
                None => None,
            },
            revisions,
        }
    }
}

/// Frozen comparison over one unchanged input universe, seed and implementation.
/// The manifest configuration must identify strategy/reference/calendar/rights
/// and population artifacts. Case configurations additionally pin economics.
pub struct EvaluationPlan<'a> {
    manifest: &'a RunManifest,
    end: PreciseInstant,
    cases: Vec<EvaluationCase>,
    bytes: Vec<u8>,
}
impl std::fmt::Debug for EvaluationPlan<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvaluationPlan")
            .field("cases", &self.cases.len())
            .finish_non_exhaustive()
    }
}
impl<'a> EvaluationPlan<'a> {
    /// Freeze 4..=16 cases with at least one case per axis. The inclusive holdout
    /// must start strictly after selection and contain the entire manifest.
    /// No optimization, outcome-based filtering, warmup or implicit defaults.
    ///
    /// # Errors
    /// Reject chronology, absent/duplicate/multi-factor cases, changed semantic
    /// model revision, changed settlement policy or final revision outside 1..=8192.
    pub fn new(
        manifest: &'a RunManifest,
        selection_end: PreciseInstant,
        holdout: (PreciseInstant, PreciseInstant),
        cases: &[EvaluationCase],
    ) -> Result<Self, EvaluationError> {
        use SensitivityAxis::{Baseline, Cost, Latency, Liquidity};
        let baseline = cases.first().ok_or(EvaluationError::Plan)?;
        if !(4..=16).contains(&cases.len()) || baseline.axis != Baseline {
            return Err(EvaluationError::Plan);
        }
        if selection_end >= holdout.0
            || holdout.0 > holdout.1
            || Run::new(manifest).now() < holdout.0
            || manifest
                .inputs()
                .iter()
                .any(|i| i.available_at() > holdout.1)
        {
            return Err(EvaluationError::Chronology);
        }
        let p = baseline.profile;
        for (index, case) in cases.iter().enumerate() {
            let q = case.profile;
            let changes = (
                p.fees_microcents() != q.fees_microcents(),
                p.latencies() != q.latencies(),
                p.participation_bps() != q.participation_bps(),
            );
            let expected = match case.axis {
                Baseline if index == 0 => (false, false, false),
                Cost => (true, false, false),
                Latency => (false, true, false),
                Liquidity => (false, false, true),
                Baseline => return Err(EvaluationError::Plan),
            };
            if changes != expected
                || q.identity().0 != p.identity().0
                || case.settlement_policy != baseline.settlement_policy
                || !(1..=8192).contains(&case.revisions)
                || cases[..index].iter().any(|old| {
                    old.profile == q
                        || old.configuration == case.configuration
                        || old.execution == case.execution
                })
            {
                return Err(EvaluationError::Plan);
            }
        }
        if [Cost, Latency, Liquidity]
            .iter()
            .any(|a| !cases.iter().any(|c| c.axis == *a))
        {
            return Err(EvaluationError::Plan);
        }
        let mut bytes = b"EA-EVALUATION-PLAN1\0".to_vec();
        bytes.extend(manifest.digest().bytes());
        for at in [selection_end, holdout.0, holdout.1] {
            let (seconds, nanos) = at.unix_parts();
            bytes.extend(seconds.to_be_bytes());
            bytes.extend(nanos.to_be_bytes());
        }
        bytes.extend((cases.len() as u64).to_be_bytes());
        for case in cases {
            bytes.push(match case.axis {
                Baseline => 0,
                Cost => 1,
                Latency => 2,
                Liquidity => 3,
            });
            bytes.extend(case.profile.identity().1.bytes());
            bytes.extend(case.configuration.bytes());
            bytes.extend(case.execution.bytes());
            bytes.push(u8::from(case.settlement.is_some()));
            if let Some(policy) = case.settlement {
                if let Some(binding) = case.settlement_policy {
                    bytes.extend(binding.bytes());
                }
                bytes.extend(policy.bytes());
            }
            bytes.extend(case.revisions.to_be_bytes());
        }
        Ok(Self {
            manifest,
            end: holdout.1,
            cases: cases.to_vec(),
            bytes,
        })
    }
    /// Versioned bytes to retain before running ANY case. Authenticating and
    /// timestamping experiment registration belongs to caller-owned storage.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Content identity of manifest, chronological partition and complete census.
    #[must_use]
    pub fn digest(&self) -> ArtifactDigest {
        ArtifactDigest::of(&self.bytes)
    }

    /// Evaluate ALL cases in declared order or return no report. Each sample
    /// couples a checked owner frame with its atomically retained P05 checkpoint.
    /// The caller authenticates exports, checkpoints and owner high-water marks.
    /// This checks replay integrity; it does not recalculate fills from snapshots.
    ///
    /// # Errors
    /// Reject missing/foreign/open histories, invalid continuations, future marks
    /// and financial drift. Maximum encoded trace size is 16 MiB per case.
    pub fn evaluate(
        &self,
        traces: &[EvaluationTrace<'_>],
    ) -> Result<EvaluationReport, EvaluationError> {
        if traces.len() != self.cases.len() {
            return Err(EvaluationError::Incomplete);
        }
        let mut metrics = Vec::with_capacity(traces.len());
        let baseline_sample = traces
            .first()
            .and_then(|t| t.samples.first())
            .ok_or(EvaluationError::Incomplete)?;
        let baseline = baseline_sample.frame.state();
        for (case, trace) in self.cases.iter().zip(traces) {
            let first = trace
                .samples
                .first()
                .ok_or(EvaluationError::Incomplete)?
                .frame
                .state();
            if trace
                .samples
                .first()
                .is_some_and(|s| s.checkpoint != baseline_sample.checkpoint)
            {
                return Err(EvaluationError::Scope);
            }
            if (
                &first.account,
                first.cash_cents,
                &first.positions,
                &first.valuations,
                &first.risk_policy,
                first.risk_revision,
                &first.risk_breach,
                &first.risk_checkpoint,
            ) != (
                &baseline.account,
                baseline.cash_cents,
                &baseline.positions,
                &baseline.valuations,
                &baseline.risk_policy,
                baseline.risk_revision,
                &baseline.risk_breach,
                &baseline.risk_checkpoint,
            ) {
                return Err(EvaluationError::Scope);
            }
            metrics.push(report::evaluate(self, case, trace)?);
        }
        EvaluationReport::new(self.digest(), metrics)
    }
}

/// One owner revision and its same-commit virtual run continuation. No mutation.
#[derive(Clone, Copy)]
pub struct EvaluationSample<'a> {
    /// Authenticated immutable owner export, including genesis at index zero.
    pub frame: &'a AccountAuditFrame,
    /// Complete retained P05 continuation, not a caller-supplied timestamp.
    pub checkpoint: &'a [u8],
}
/// Complete finite history for a single predeclared case.
pub struct EvaluationTrace<'a> {
    /// Genesis through final revision, exactly once and in contiguous order.
    pub samples: &'a [EvaluationSample<'a>],
    /// Authenticated owner high-water; must equal the predeclared final revision.
    pub owner_revision: u64,
}
impl std::fmt::Debug for EvaluationSample<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EvaluationSample(..)")
    }
}
impl std::fmt::Debug for EvaluationTrace<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EvaluationTrace(..)")
    }
}
