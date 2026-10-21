//! Account-owned deterministic execution. No public provider/authentication path.
mod bootstrap;
mod fence;
mod handling;
pub(crate) mod persistence;

pub use bootstrap::{InitialPositionCost, provision_execution};

use super::{AdmissionContext, invariant, permanent};
use edgeagent_market_data_domain::{PreciseInstant, ReadyEvidence};
use edgeagent_messaging::HandlerFailure;
use edgeagent_simulation_domain::{
    ArtifactDigest, Run,
    execution::{ExecutableLiquidity, ExecutionProfile},
};

/// Explicit action at one canonical virtual-time slot. Pending cancel does not
/// release anything; confirmation is a separate, durably ordered operation.
#[derive(Debug)]
pub enum ExecutionAction<'evidence> {
    /// Attempt using P04 evidence and verified retained observation digests.
    Attempt(ReadyEvidence<'evidence>, Vec<ArtifactDigest>),
    /// Request cancellation; fills remain possible until confirmation.
    RequestCancel,
    /// Confirm after the configured cancellation delay.
    ConfirmCancel,
    /// Expire at/after the order's explicit deadline.
    Expire,
}
/// Owner-authored finite schedule element; requests cannot change these terms.
#[derive(Debug)]
pub struct ExecutionStep<'evidence> {
    sequence: u64,
    quote: ArtifactDigest,
    pub(crate) at: PreciseInstant,
    action: ExecutionAction<'evidence>,
}
impl<'evidence> ExecutionStep<'evidence> {
    /// Bind an action to a reservation catalog entry and canonical sequence/time.
    /// The context checks contiguous combined scheduling and frozen provenance.
    #[must_use]
    pub const fn new(
        sequence: u64,
        quote: ArtifactDigest,
        at: PreciseInstant,
        action: ExecutionAction<'evidence>,
    ) -> Self {
        Self {
            sequence,
            quote,
            at,
            action,
        }
    }
}
/// Verified-composition capability; bind the authenticated principal through the
/// admission context, never through a payload/source URI. Retain the plan and
/// its evidence artifacts with the run. No defaults or mutable model settings.
pub struct ExecutionContext<'context, 'run, 'evidence> {
    pub(crate) admission: &'context AdmissionContext<'run>,
    pub(crate) profile: ExecutionProfile,
    pub(crate) steps: Vec<ExecutionStep<'evidence>>,
    pub(crate) binding: ArtifactDigest,
    canonical: Vec<u8>,
}
impl<'context, 'run, 'evidence> ExecutionContext<'context, 'run, 'evidence> {
    /// Validate and fingerprint a complete bounded plan before any account work.
    /// P08 synthetic price/fee caps remain mandatory conservative protections;
    /// actual prices/size come exclusively from consumed ready evidence.
    ///
    /// # Errors
    /// Reject missing/mismatched slots, chronology, fee caps or evidence.
    pub fn new(
        admission: &'context AdmissionContext<'run>,
        profile: ExecutionProfile,
        mut steps: Vec<ExecutionStep<'evidence>>,
    ) -> Result<Self, HandlerFailure> {
        steps.sort_by_key(|step| step.sequence);
        if steps.is_empty() || steps.len() > 4096 || steps.len() != admission.execution_slots.len()
        {
            return Err(invariant());
        }
        let mut slots = admission.execution_slots.clone();
        slots.sort_by_key(|slot| slot.0);
        let mut bytes = b"EA-EXECUTION-PLAN1\0".to_vec();
        bytes.extend(admission.configuration.bytes());
        bytes.extend(profile.identity().1.bytes());
        for (step, slot) in steps.iter().zip(slots) {
            if (step.sequence, step.at) != slot {
                return Err(invariant());
            }
            let quote = admission
                .quotes
                .iter()
                .find(|q| q.fingerprint() == step.quote)
                .ok_or_else(invariant)?;
            if step.sequence <= quote.position().0
                || step.at < quote.position().1
                || profile
                    .fee_cents(quote.order().terms().2.parts().1)
                    .map_err(|_| invariant())?
                    > quote.protection().1.get()
            {
                return Err(permanent("execution_configuration"));
            }
            bytes.extend(step.sequence.to_be_bytes());
            bytes.extend(step.quote.bytes());
            let (s, n) = step.at.unix_parts();
            bytes.extend(s.to_be_bytes());
            bytes.extend(n.to_be_bytes());
            match &step.action {
                ExecutionAction::Attempt(ready, artifacts) => {
                    bytes.push(0);
                    let mut run = Run::new(admission.manifest);
                    while run.peek().is_some_and(|i| i.available_at() <= step.at) {
                        run.next_input();
                    }
                    run.advance_to(step.at).map_err(|_| invariant())?;
                    let liquidity = ExecutableLiquidity::from_ready(
                        &run,
                        quote.order(),
                        profile,
                        ready,
                        artifacts,
                    )
                    .map_err(|_| permanent("execution_evidence"))?;
                    let (key, content, capacity) = liquidity.budget();
                    bytes.extend(key.bytes());
                    bytes.extend(content.bytes());
                    bytes.extend(capacity.to_be_bytes());
                    bytes.extend((artifacts.len() as u64).to_be_bytes());
                    for artifact in artifacts {
                        bytes.extend(artifact.bytes());
                    }
                }
                ExecutionAction::RequestCancel => bytes.push(1),
                ExecutionAction::ConfirmCancel => bytes.push(2),
                ExecutionAction::Expire => bytes.push(3),
            }
        }
        if bytes.len() > 1_048_576 {
            return Err(permanent("execution_configuration"));
        }
        Ok(Self {
            admission,
            profile,
            steps,
            binding: ArtifactDigest::of(&bytes),
            canonical: bytes,
        })
    }
    /// Exact immutable execution plan identity for identity-only commands.
    #[must_use]
    pub const fn binding(&self) -> ArtifactDigest {
        self.binding
    }
}
impl std::fmt::Debug for ExecutionContext<'_, '_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("steps", &self.steps.len())
            .finish_non_exhaustive()
    }
}
