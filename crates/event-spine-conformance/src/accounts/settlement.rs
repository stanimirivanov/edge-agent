//! Invented final fixings and a finite, retained postlude. No provider authority.
use super::*;
use edgeagent_contracts::settlement::{DEFINITIONS, SettleDryRunAccount};
use edgeagent_simulation_domain::settlement::*;
use edgeagent_simulation_postgres::{
    ExecutionContext, SettlementContext, SettlementLimits, SettlementStep,
};
impl Scenario {
    /// Rehydrate a retained journal with its exact qualification catalog definition.
    pub fn restore_order(&self, index: usize, bytes: &[u8]) -> Result<Order, Error> {
        let quote = self.quotes.get(index).ok_or("order absent")?;
        Ok(Order::restore_journal(
            quote.order().clone(),
            4096,
            quote.fingerprint(),
            bytes,
        )?)
    }
    /// Whether the scenario qualifies the P10 synthetic account postlude.
    #[must_use]
    pub fn has_settlement(&self) -> bool {
        self.case.starts_with("execution-settlement")
    }
    /// Reconstruct identical final evidence and plans inside independent workers.
    pub fn settlement<'c, 'a, 'r>(
        &self,
        execution: &'c ExecutionContext<'a, 'r, 'r>,
    ) -> Result<SettlementContext<'c, 'a, 'r, 'r>, Error> {
        let policy = SettlementPolicy::new(
            option_for(&self.case)?,
            rules()?,
            SourceId::new(99)?,
            at(120)?,
            at(130)?,
            SettlementRounding::NearestEven,
            SettlementCharges::new(
                FeeBasis::EveryContract,
                if self.case.ends_with("unfunded") {
                    10_000
                } else {
                    1
                },
            ),
        )?;
        let level = if self.case.ends_with("fractional") {
            IndexPoints::new(10_000_005, 5)?
        } else {
            let put = self.case.contains("-put-");
            let value = if self.case.ends_with("-atm")
                || self.case.ends_with("-empty")
                || self.case.ends_with("-unfunded")
            {
                100
            } else if self.case.ends_with("-itm") == put {
                95
            } else {
                105
            };
            IndexPoints::new(value, 0)?
        };
        let fixing = SyntheticFinalFixing::new(&policy, ObservationId::new(99)?, at(125)?, level)?;
        let steps = vec![
            SettlementStep::new(6, at(121)?, policy.clone(), vec![]),
            SettlementStep::new(7, at(122)?, policy.clone(), vec![fixing.clone()]),
            SettlementStep::new(8, at(125)?, policy.clone(), vec![fixing.clone()]),
            SettlementStep::new(9, at(126)?, policy, vec![fixing]),
        ];
        Ok(SettlementContext::new(
            execution,
            steps,
            SettlementLimits::new(2, 2)?,
        )?)
    }
    /// Identity-only command; altered optimistic preconditions exercise conflict rejection.
    pub fn settlement_envelope(
        &self,
        context: &SettlementContext<'_, '_, '_, '_>,
        sequence: u64,
        id: &str,
        conflict: bool,
    ) -> Result<MessageEnvelope, Error> {
        let account = format!("{:032x}", 1);
        let request = SettleDryRunAccount::new(
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
                message_type: DEFINITIONS[0].message_type().into(),
                subject: format!("account/{account}"),
                time: "1970-01-01T00:00:01Z".into(),
                data_schema: DEFINITIONS[0].data_schema().into(),
                correlation_id: "p10-synthetic".into(),
                causation_id: "p10-synthetic".into(),
                idempotency_key: format!("settle:{sequence}"),
                partition_key: format!("account/{account}"),
                trace_parent: "00-00000000000000000000000000000001-0000000000000001-01".into(),
                trace_state: None,
            },
            &request,
        )?)
    }
}
pub(super) fn option_for(case: &str) -> Result<ReferenceVersion<OptionSpecification>, Error> {
    let original = option()?;
    if !case.contains("-put-") {
        return Ok(original);
    }
    let spec = original.value();
    let (_, strike) = spec.terms().payoff();
    let (tick, multiplier, _) = spec.terms().quotation();
    Ok(ReferenceVersion::new(
        original.id(),
        original.revision(),
        original.source(),
        original.known_at(),
        *original.checksum(),
        OptionSpecification::new(
            spec.id(),
            spec.underlying(),
            OptionTerms::new(
                OptionKind::Put,
                strike,
                tick,
                multiplier,
                ExerciseStyle::European,
                SettlementStyle::Cash,
            )?,
            spec.schedule(),
            spec.effective(),
        )?,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settlement_scenarios_reconstruct_identical_postludes_without_a_database() -> Result<(), Error>
    {
        for case in [
            "execution-settlement-call-itm",
            "execution-settlement-call-atm",
            "execution-settlement-call-otm",
            "execution-settlement-put-itm",
            "execution-settlement-put-atm",
            "execution-settlement-put-otm",
            "execution-settlement-fractional",
            "execution-settlement-empty",
            "execution-settlement-unfunded",
        ] {
            let scenario = Scenario::new(case)?;
            let owner = scenario.context(1)?;
            let execution = scenario.execution(&owner)?;
            let first = scenario.settlement(&execution)?;
            let second = scenario.settlement(&execution)?;
            assert_eq!(first.binding(), second.binding());
        }
        Ok(())
    }
}
