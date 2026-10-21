//! Explicit invented P09 economics and finite schedule, never runtime defaults.
use super::*;
use edgeagent_contracts::execution::{DEFINITIONS as EXECUTION, ExecuteDryRunStep};
use edgeagent_simulation_domain::execution::{ExecutionProfile, PositionCost};
use edgeagent_simulation_postgres::{
    ExecutionAction, ExecutionContext, ExecutionStep, InitialPositionCost,
};
impl Scenario {
    /// Whether this fixture includes qualified synthetic execution, not just admission.
    #[must_use]
    pub fn has_execution(&self) -> bool {
        self.case.starts_with("execution")
    }
    /// Build an identical plan in every independent worker; evidence is borrowed
    /// from the original tracker, never forged from wire metadata.
    pub fn execution<'context, 'run>(
        &'run self,
        context: &'context AdmissionContext<'run>,
    ) -> Result<ExecutionContext<'context, 'run, 'run>, Error> {
        if !self.has_execution() {
            return Err("not an execution fixture".into());
        }
        let profile = ExecutionProfile::new(
            rules()?,
            10_000,
            500_000,
            250_000,
            Duration::from_secs(1),
            Duration::from_secs(2),
        )?;
        let first = self.quotes.first().ok_or("first quote")?.fingerprint();
        let second = self.quotes.get(1).ok_or("second quote")?.fingerprint();
        let mut steps = Vec::new();
        for (sequence, second_at) in [(3, 1), (4, 2), (5, 2), (6, 2), (7, 3), (8, 4), (9, 100)] {
            let second_at = if self.case == "execution-full" && sequence == 4 {
                1
            } else {
                second_at
            };
            let quote = if sequence == 6 || sequence == 9 {
                second
            } else {
                first
            };
            let (tracker, observation, artifact) = if second_at >= 2 {
                (
                    self.following
                        .as_ref()
                        .ok_or("post-activation evidence missing")?,
                    2,
                    ArtifactDigest::of(b"post-activation-evidence-v1"),
                )
            } else {
                (
                    &self.tracker,
                    1,
                    ArtifactDigest::of(b"invented-share-trade-v1"),
                )
            };
            let action = match (self.case == "execution-full", sequence) {
                (true, 4) | (false, 5) => ExecutionAction::RequestCancel,
                (true, 7) | (_, 8) => ExecutionAction::ConfirmCancel,
                (_, 9) => ExecutionAction::Expire,
                _ => ExecutionAction::Attempt(
                    self.readiness.check(
                        at(second_at)?,
                        &[EvidenceInput::new(
                            tracker,
                            FeedEpoch::new(1)?,
                            ObservationId::new(observation)?,
                            ObservationRevision::new(1)?,
                        )],
                    )?,
                    vec![artifact],
                ),
            };
            steps.push(ExecutionStep::new(sequence, quote, at(second_at)?, action));
        }
        Ok(ExecutionContext::new(context, profile, steps)?)
    }
    /// Explicit synthetic pre-existing basis, distinct from the risk mark.
    pub fn costs(&self) -> Result<Vec<InitialPositionCost>, Error> {
        let mut costs = std::collections::BTreeMap::new();
        for quote in &self.quotes {
            let order = quote.order();
            let strategy = format!("{:032x}", order.scope().identity().2.get());
            let instrument = format!("{:032x}", order.instrument().id().get());
            let unit = if self.case == "execution-options" {
                "contracts"
            } else {
                "shares"
            };
            let position = if self.case == "execution-sell" {
                PositionCost::new(5, 25_000)?
            } else {
                PositionCost::new(0, 0)?
            };
            costs.insert(
                (strategy.clone(), instrument.clone(), unit),
                InitialPositionCost {
                    strategy,
                    instrument,
                    unit: unit.into(),
                    position,
                },
            );
        }
        Ok(costs.into_values().collect())
    }
    /// Exact plan intent with replayable preconditions; no caller amounts/time.
    pub fn execution_envelope(
        &self,
        context: &ExecutionContext<'_, '_, '_>,
        sequence: u64,
        id: &str,
        conflict: bool,
    ) -> Result<MessageEnvelope, Error> {
        let account = format!("{:032x}", 1);
        let request = ExecuteDryRunStep::new(
            "dry_run",
            &account,
            &hex(&context.binding().bytes()),
            sequence,
            if conflict { 99 } else { sequence - 1 },
        )?;
        Ok(MessageEnvelope::from_payload(
            MessageMetadata {
                id: id.into(),
                source: Component::Gateway.source_uri().into(),
                message_type: EXECUTION[0].message_type().into(),
                subject: format!("account/{account}"),
                time: "1970-01-01T00:00:01Z".into(),
                data_schema: EXECUTION[0].data_schema().into(),
                correlation_id: "p09-synthetic".into(),
                causation_id: "p09-synthetic".into(),
                idempotency_key: format!("step:{sequence}"),
                partition_key: format!("account/{account}"),
                trace_parent: "00-00000000000000000000000000000001-0000000000000001-01".into(),
                trace_state: None,
            },
            &request,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_execution_fixture_has_a_valid_frozen_plan_without_a_database() -> Result<(), Error> {
        for case in [
            "execution",
            "execution-sell",
            "execution-options",
            "execution-full",
            "execution-denied",
        ] {
            let scenario =
                Scenario::new(case).map_err(|error| format!("scenario {case}: {error}"))?;
            let admission = scenario
                .context(1)
                .map_err(|error| format!("admission {case}: {error}"))?;
            scenario
                .execution(&admission)
                .map_err(|error| format!("execution {case}: {error}"))?;
        }
        Ok(())
    }
}
