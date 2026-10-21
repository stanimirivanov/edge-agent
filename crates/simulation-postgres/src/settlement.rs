//! Finite canonical settlement postlude; ownership, persistence and accounting
//! effects stay inside the existing inbox transaction. No public activation.
mod bootstrap;
mod fence;
mod handling;
mod orders;
use crate::{ExecutionContext, invariant, permanent};
pub use bootstrap::provision_settlement;
use edgeagent_market_data_domain::PreciseInstant;
use edgeagent_messaging::HandlerFailure;
use edgeagent_simulation_domain::{ArtifactDigest, execution::PositionCost, settlement::*};

/// Explicit work bounds; the emitted fact also faces a strict 16 KiB byte limit.
#[derive(Clone, Copy, Debug)]
pub struct SettlementLimits {
    orders: usize,
    allocations: usize,
}
impl SettlementLimits {
    /// Require both limits in 1..=32; no truncation or guessed defaults.
    /// # Errors
    /// Reject zero/excessive operation or attribution work.
    pub fn new(orders: usize, allocations: usize) -> Result<Self, HandlerFailure> {
        if !(1..=32).contains(&orders) || !(1..=32).contains(&allocations) {
            return Err(invariant());
        }
        Ok(Self {
            orders,
            allocations,
        })
    }
}
/// One owner-authored operation; no caller supplies evidence or financial values.
#[derive(Debug)]
pub struct SettlementStep {
    sequence: u64,
    at: PreciseInstant,
    policy: SettlementPolicy,
    fixings: Vec<SyntheticFinalFixing>,
}
impl SettlementStep {
    /// Bind injected virtual time, explicit policy and the complete candidate set.
    #[must_use]
    pub fn new(
        sequence: u64,
        at: PreciseInstant,
        policy: SettlementPolicy,
        fixings: Vec<SyntheticFinalFixing>,
    ) -> Self {
        Self {
            sequence,
            at,
            policy,
            fixings,
        }
    }
}
/// Verified principal comes from the borrowed execution/admission composition.
/// Retain and reconstruct identical artifacts after restart; never infer a fixing.
pub struct SettlementContext<'context, 'admission, 'run, 'evidence> {
    execution: &'context ExecutionContext<'admission, 'run, 'evidence>,
    steps: Vec<SettlementStep>,
    limits: SettlementLimits,
    canonical: Vec<u8>,
    binding: ArtifactDigest,
}
impl<'context, 'admission, 'run, 'evidence>
    SettlementContext<'context, 'admission, 'run, 'evidence>
{
    /// Append contiguous slots after the complete finite admission/execution plan.
    /// Original frozen market inputs must finish before settlement can run. Limits,
    /// policies, candidate bytes and all times are bound by the retained plan.
    /// # Errors
    /// Reject chronology, unsupported/foreign specifications, ambiguity, changing
    /// policy/final fixing for one instrument, or a plan above 4096 slots/1 MiB.
    pub fn new(
        execution: &'context ExecutionContext<'admission, 'run, 'evidence>,
        mut steps: Vec<SettlementStep>,
        limits: SettlementLimits,
    ) -> Result<Self, HandlerFailure> {
        let owner = execution.admission;
        let base = owner.quotes.len() + execution.steps.len();
        if steps.is_empty() || base.saturating_add(steps.len()) > 4096 {
            return Err(invariant());
        }
        steps.sort_by_key(|s| s.sequence);
        let last = execution
            .steps
            .iter()
            .map(|s| s.at)
            .chain(owner.quotes.iter().map(|q| q.position().1))
            .chain(owner.manifest.inputs().iter().map(|i| i.available_at()))
            .max()
            .ok_or_else(invariant)?;
        let mut previous = last;
        let mut policies = std::collections::BTreeMap::new();
        let mut observations = std::collections::BTreeMap::new();
        let mut canonical = b"EA-ACCOUNT-SETTLEMENT-POSTLUDE1\0".to_vec();
        canonical.extend(execution.binding.bytes());
        canonical.extend((limits.orders as u64).to_be_bytes());
        canonical.extend((limits.allocations as u64).to_be_bytes());
        canonical.extend((steps.len() as u64).to_be_bytes());
        for (i, step) in steps.iter().enumerate() {
            let spec = step.policy.specification();
            let id = spec.value().id().get();
            if step.sequence != (base + i + 1) as u64
                || step.at < previous
                || step.fixings.len() > 1
            {
                return Err(invariant());
            }
            previous = step.at;
            let quotes: Vec<_> = owner
                .quotes
                .iter()
                .filter(|q| q.order().instrument().id().get() == id)
                .collect();
            if quotes.is_empty()
                || quotes.len() > limits.orders
                || quotes
                    .iter()
                    .any(|q| q.order().instrument().option_specification() != Some(spec))
            {
                return Err(permanent("settlement_specification"));
            }
            let identity = step.policy.identity().bytes();
            if policies.insert(id, identity).is_some_and(|p| p != identity) {
                return Err(invariant());
            }
            // Validate future evidence without consulting wall time or selecting a
            // revision. Empty lots need final evidence too; no dummy position.
            let known = step
                .policy
                .timing()
                .1
                .max(PreciseInstant::from_millis(
                    spec.value().schedule().settlement(),
                ))
                .max(PreciseInstant::from_millis(spec.known_at()));
            step.policy
                .calculate_account(
                    known,
                    PositionCost::new(0, 0).map_err(|_| invariant())?,
                    0,
                    &step.fixings,
                )
                .map_err(|_| permanent("settlement_evidence"))?;
            canonical.extend(step.sequence.to_be_bytes());
            let (s, n) = step.at.unix_parts();
            canonical.extend(s.to_be_bytes());
            canonical.extend(n.to_be_bytes());
            let bytes = step.policy.canonical_bytes();
            canonical.extend((bytes.len() as u64).to_be_bytes());
            canonical.extend(bytes);
            canonical.push(u8::from(!step.fixings.is_empty()));
            for fixing in &step.fixings {
                let digest = fixing.identity().bytes();
                if observations.insert(id, digest).is_some_and(|v| v != digest) {
                    return Err(invariant());
                }
                let bytes = fixing.canonical_bytes();
                canonical.extend((bytes.len() as u64).to_be_bytes());
                canonical.extend(bytes);
            }
        }
        if canonical.len() > 1_048_576 {
            return Err(invariant());
        }
        Ok(Self {
            execution,
            steps,
            limits,
            binding: ArtifactDigest::of(&canonical),
            canonical,
        })
    }
    /// Immutable identity for commands and retained configuration checks.
    #[must_use]
    pub const fn binding(&self) -> ArtifactDigest {
        self.binding
    }
}
impl std::fmt::Debug for SettlementContext<'_, '_, '_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SettlementContext(..)")
    }
}
