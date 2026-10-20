use edgeagent_market_data_domain::{
    ObservationValue, PreciseInstant, ReadyEvidence, SemanticReference,
};

use super::{
    FillFact, Order, OrderDefinition, OrderError, OrderEvent, OrderEventId, OrderEventKind,
    OrderPrice, OrderQuantity,
};
use crate::{ArtifactDigest, FrozenInput, Run};

/// Incremental model output. The order independently checks units, limit and size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProposedFill {
    pub(super) quantity: OrderQuantity,
    pub(super) price: OrderPrice,
}
impl ProposedFill {
    /// Propose, but do not authorize or apply, a positive quantity at a typed price.
    #[must_use]
    pub const fn new(quantity: OrderQuantity, price: OrderPrice) -> Self {
        Self { quantity, price }
    }
    /// Incremental quantity and execution price.
    #[must_use]
    pub const fn values(self) -> (OrderQuantity, OrderPrice) {
        (self.quantity, self.price)
    }
}

/// Pure simulation-only boundary consumed by the order proposal coordinator.
/// Implementations must not perform I/O, read wall time or acquire hidden entropy.
/// P09 qualifies fill/latency/liquidity algorithms and their explicit RNG schedule;
/// this port conveys no broker capability and promises no realistic fills.
pub trait SimulationExecution: Send {
    /// Pinned implementation revision and configuration artifact digest.
    fn identity(&self) -> (SemanticReference, ArtifactDigest);
    /// Compute a proposal from read-only qualified inputs; `None` means no fill.
    ///
    /// # Errors
    /// Return a payload-free failure if computation cannot produce a valid result.
    /// A failure is not a fill and must not advance the order journal.
    fn propose(
        &mut self,
        request: SimulationRequest<'_, '_>,
    ) -> Result<Option<ProposedFill>, OrderError>;
}

/// Read-only request; models cannot choose fact identities or advance virtual time.
#[derive(Debug)]
pub struct SimulationRequest<'borrow, 'tracker> {
    order: &'borrow OrderDefinition,
    remaining: OrderQuantity,
    at: PreciseInstant,
    evidence: &'borrow ReadyEvidence<'tracker>,
}
impl<'borrow, 'tracker> SimulationRequest<'borrow, 'tracker> {
    /// Immutable order terms, including ownership and product specification.
    #[must_use]
    pub const fn order(&self) -> &'borrow OrderDefinition {
        self.order
    }
    /// Remaining executable quantity before this proposal, including pending cancel.
    #[must_use]
    pub const fn remaining(&self) -> OrderQuantity {
        self.remaining
    }
    /// Explicit virtual simulation time.
    #[must_use]
    pub const fn at(&self) -> PreciseInstant {
        self.at
    }
    /// Borrowed ready barrier; no cloning/forging of readiness authority.
    #[must_use]
    pub const fn evidence(&self) -> &'borrow ReadyEvidence<'tracker> {
        self.evidence
    }
}

/// Caller-assigned attempt identity and pinned model/evidence claims.
/// P08 must atomically retain the resulting fact and P05 continuation. Artifact
/// hashes require caller verification; they do not certify data rights or quality.
#[derive(Clone, Copy, Debug)]
pub struct SimulationStep<'borrow, 'tracker> {
    id: OrderEventId,
    revision: u64,
    ready: &'borrow ReadyEvidence<'tracker>,
    model: (SemanticReference, ArtifactDigest),
    evidence: ArtifactDigest,
    artifacts: &'borrow [ArtifactDigest],
}
impl<'borrow, 'tracker> SimulationStep<'borrow, 'tracker> {
    /// Bind one deterministic attempt to ready evidence and expected model identity.
    ///
    /// # Errors
    /// Returns `Revision` for zero. Time is derived from the ready assessment.
    pub fn new(
        id: OrderEventId,
        revision: u64,
        ready: &'borrow ReadyEvidence<'tracker>,
        model: (SemanticReference, ArtifactDigest),
        evidence: ArtifactDigest,
        artifacts: &'borrow [ArtifactDigest],
    ) -> Result<Self, OrderError> {
        if revision == 0 {
            return Err(OrderError::Revision);
        }
        Ok(Self {
            id,
            revision,
            ready,
            model,
            evidence,
            artifacts,
        })
    }
}

impl Order {
    /// Produce an uncommitted fact; call `apply` only after authoritative commit.
    /// Retained attempts (including no-fill) return their exact fact without model
    /// execution. New attempts require matching run/time/product evidence and pass
    /// lifecycle checks before calling the pure model. Invalid output changes no
    /// order state. Any model continuation still belongs in P08's atomic commit.
    ///
    /// # Errors
    /// Returns `Evidence`, `ModelMismatch`, identity/revision/time/state/capacity
    /// errors, model failures or invalid proposal units, prices and quantities.
    pub fn simulate(
        &self,
        run: &Run<'_>,
        model: &mut dyn SimulationExecution,
        step: SimulationStep<'_, '_>,
    ) -> Result<OrderEvent, OrderError> {
        let at = step.ready.assessment().0;
        if run.manifest_digest() != self.definition.scope.run || run.now() != at {
            return Err(OrderError::Evidence);
        }
        let frozen = FrozenInput::from_ready(step.ready, step.artifacts)
            .map_err(|_| OrderError::Evidence)?;
        if !frozen.iter().all(|input| {
            run.consumed_inputs()
                .iter()
                .any(|retained| retained.supports_reassessment(input))
        }) {
            return Err(OrderError::Evidence);
        }
        if model.identity() != step.model {
            return Err(OrderError::ModelMismatch);
        }
        if let Some(event) = self.retained(step.id) {
            let provenance = match event.kind {
                OrderEventKind::Filled(fill) => (fill.model, fill.evidence),
                OrderEventKind::NoFill { model, evidence } => (model, evidence),
                _ => return Err(OrderError::IdentityConflict),
            };
            return if event.revision == step.revision
                && event.at == at
                && provenance == (step.model, step.evidence)
            {
                Ok(*event)
            } else {
                Err(OrderError::IdentityConflict)
            };
        }
        self.check_execution(step.revision, at)?;
        let instrument = &self.definition.instrument;
        let matching = step.ready.observations().iter().any(|observation| {
            if observation.context().origin().0 != instrument.id() {
                return false;
            }
            match (instrument.specification(), observation.value()) {
                (None, Some(ObservationValue::Trade(_) | ObservationValue::Bar(_))) => true,
                (Some(spec), Some(ObservationValue::OptionQuote(quote))) => {
                    quote.specification() == spec
                }
                _ => false,
            }
        });
        if !matching {
            return Err(OrderError::Evidence);
        }
        let remaining = self.quantities().1.ok_or(OrderError::State)?;
        let proposal = model.propose(SimulationRequest {
            order: &self.definition,
            remaining,
            at,
            evidence: step.ready,
        })?;
        let kind = proposal.map_or(
            OrderEventKind::NoFill {
                model: step.model,
                evidence: step.evidence,
            },
            |proposal| OrderEventKind::Filled(FillFact::new(proposal, step.model, step.evidence)),
        );
        let event = OrderEvent::new(
            step.id,
            self.definition.scope.order,
            step.revision,
            at,
            kind,
        )?;
        self.project(event)?;
        Ok(event)
    }
}
