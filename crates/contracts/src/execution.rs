//! Service-owned finite dry-run execution steps. Requests contain no financial
//! amounts, evidence, actor, execution time or provider-controlled model settings.
use crate::{Component, EventRetention, MessageDefinition, MessageRegistry, MessageRoutingError};
use serde::{Deserialize, Serialize};

/// Execute a registered step and publish its committed account/order transition.
pub const DEFINITIONS: [MessageDefinition; 2] = [
    MessageDefinition::command(
        "com.edgeagent.simulation.execute-step.v1",
        "urn:edgeagent:schema:simulation:execute-step:v1",
        Component::ExecutionSimulator,
        "account",
    ),
    MessageDefinition::event(
        "com.edgeagent.simulation.execution-decided.v1",
        "urn:edgeagent:schema:simulation:execution-decided:v1",
        Component::ExecutionSimulator,
        "account",
        EventRetention::Audit,
    ),
];
/// Immutable v1 command schema, resolved locally rather than fetched from its URI.
pub const STEP_SCHEMA: &str = include_str!("../schemas/simulation/execute-step-v1.json");
/// Immutable v1 committed execution-decision schema.
pub const DECISION_SCHEMA: &str = include_str!("../schemas/simulation/execution-decided-v1.json");
/// Execution registry; application composition may union it with admission.
pub fn registry() -> Result<MessageRegistry<'static>, MessageRoutingError> {
    MessageRegistry::new(&DEFINITIONS)
}

/// Payload-free contract validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionContractError;
impl std::fmt::Display for ExecutionContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid dry-run execution contract")
    }
}
impl std::error::Error for ExecutionContractError {}

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct StepPayload {
    mode: String,
    account: String,
    plan: String,
    sequence: u64,
    expected_revision: u64,
}
/// Identity-only intent referencing an immutable owner-authored execution plan.
#[derive(Clone, Serialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct ExecuteDryRunStep(StepPayload);
impl<'de> Deserialize<'de> for ExecuteDryRunStep {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let p = StepPayload::deserialize(d)?;
        Self::new(
            &p.mode,
            &p.account,
            &p.plan,
            p.sequence,
            p.expected_revision,
        )
        .map_err(serde::de::Error::custom)
    }
}
impl ExecuteDryRunStep {
    /// Require exact mode, lowercase fixed nonzero identities and SQL-sized revisions.
    ///
    /// # Errors
    /// Returns a payload-free error for malformed/unsupported input.
    pub fn new(
        mode: &str,
        account: &str,
        plan: &str,
        sequence: u64,
        expected_revision: u64,
    ) -> Result<Self, ExecutionContractError> {
        if mode != "dry_run"
            || !hex(account, 32)
            || !hex(plan, 64)
            || sequence == 0
            || sequence > i64::MAX as u64
            || expected_revision >= i64::MAX as u64
        {
            return Err(ExecutionContractError);
        }
        Ok(Self(StepPayload {
            mode: mode.into(),
            account: account.into(),
            plan: plan.into(),
            sequence,
            expected_revision,
        }))
    }
    /// Account, immutable plan, canonical sequence and expected account revision.
    #[must_use]
    pub fn position(&self) -> (&str, &str, u64, u64) {
        (
            &self.0.account,
            &self.0.plan,
            self.0.sequence,
            self.0.expected_revision,
        )
    }
}
impl std::fmt::Debug for ExecuteDryRunStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExecuteDryRunStep(..)")
    }
}

/// Immutable committed execution fact. Values are incremental except explicitly
/// cumulative revisions. Account/order identity and ordering remain authoritative;
/// this fact does not authorize any live execution or change portfolio projections.
#[derive(Clone, Serialize, Eq, PartialEq)]
pub struct ExecutionDecision {
    request: ExecuteDryRunStep,
    order: String,
    order_revision: u64,
    status: String,
    filled_units: u64,
    cash_delta_cents: i64,
    fee_delta_cents: u64,
    position_cost_delta_cents: i64,
    realized_pnl_expense_cents: i64,
    strategy: String,
    instrument: String,
    unit: String,
    side: String,
    model: String,
    evidence: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionPayload {
    request: ExecuteDryRunStep,
    order: String,
    order_revision: u64,
    status: String,
    filled_units: u64,
    cash_delta_cents: i64,
    fee_delta_cents: u64,
    position_cost_delta_cents: i64,
    realized_pnl_expense_cents: i64,
    strategy: String,
    instrument: String,
    unit: String,
    side: String,
    model: String,
    #[serde(deserialize_with = "required_evidence")]
    evidence: Option<String>,
}
fn required_evidence<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}
impl<'de> Deserialize<'de> for ExecutionDecision {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let p = DecisionPayload::deserialize(d)?;
        Self::new(
            p.request,
            &p.order,
            p.order_revision,
            &p.status,
            (
                p.filled_units,
                [
                    p.cash_delta_cents,
                    p.position_cost_delta_cents,
                    p.realized_pnl_expense_cents,
                    i64::try_from(p.fee_delta_cents).map_err(serde::de::Error::custom)?,
                ],
            ),
            (&p.model, p.evidence.as_deref()),
            (&p.strategy, &p.instrument, &p.unit, &p.side),
        )
        .map_err(serde::de::Error::custom)
    }
}
impl ExecutionDecision {
    /// Construct a checked result after domain/account validation, before atomic
    /// persistence. Status is one of the explicit v1 lifecycle effects.
    /// Amounts are filled units and balanced signed debits in order: cash,
    /// position cost, realized P/L expense, fee expense. Allocation is strategy,
    /// instrument, whole-unit kind and long-only side. No float or inferred basis.
    ///
    /// # Errors
    /// Returns a payload-free error for invalid identities/status/revisions.
    pub fn new(
        request: ExecuteDryRunStep,
        order: &str,
        order_revision: u64,
        status: &str,
        amounts: (u64, [i64; 4]),
        provenance: (&str, Option<&str>),
        allocation: (&str, &str, &str, &str),
    ) -> Result<Self, ExecutionContractError> {
        let (model, evidence) = provenance;
        let (strategy, instrument, unit, side) = allocation;
        let [cash, position, realized, fee] = amounts.1;
        if !hex(order, 64)
            || !hex(model, 64)
            || order_revision == 0
            || order_revision > i64::MAX as u64
            || evidence.is_some_and(|e| !hex(e, 64))
            || !matches!(
                status,
                "filled" | "partial_fill" | "no_fill" | "cancel_pending" | "cancelled" | "expired"
            )
            || ((status == "filled" || status == "partial_fill") != (amounts.0 > 0))
            || amounts.0 > i64::MAX as u64
            || fee < 0
            || amounts.1.iter().map(|n| i128::from(*n)).sum::<i128>() != 0
            || (amounts.0 == 0 && amounts.1 != [0; 4])
            || !hex(strategy, 32)
            || !hex(instrument, 32)
            || !matches!(unit, "shares" | "contracts")
            || !matches!(side, "buy_to_open" | "sell_to_close")
            || (matches!(status, "filled" | "partial_fill" | "no_fill") != evidence.is_some())
        {
            return Err(ExecutionContractError);
        }
        Ok(Self {
            request,
            order: order.into(),
            order_revision,
            status: status.into(),
            filled_units: amounts.0,
            cash_delta_cents: cash,
            fee_delta_cents: u64::try_from(fee).map_err(|_| ExecutionContractError)?,
            position_cost_delta_cents: position,
            realized_pnl_expense_cents: realized,
            strategy: strategy.into(),
            instrument: instrument.into(),
            unit: unit.into(),
            side: side.into(),
            model: model.into(),
            evidence: evidence.map(str::to_owned),
        })
    }
    /// Original intent, order identity and resulting contiguous order revision.
    #[must_use]
    pub fn position(&self) -> (&ExecuteDryRunStep, &str, u64) {
        (&self.request, &self.order, self.order_revision)
    }
    /// Lifecycle decision and incremental filled quantity.
    #[must_use]
    pub fn effect(&self) -> (&str, u64) {
        (&self.status, self.filled_units)
    }
    /// Strategy, instrument, shares/contracts and buy-to-open/sell-to-close.
    #[must_use]
    pub fn allocation(&self) -> (&str, &str, &str, &str) {
        (&self.strategy, &self.instrument, &self.unit, &self.side)
    }
    /// Balanced debits: cash, position cost, realized P/L expense, fee expense.
    #[must_use]
    pub fn postings(&self) -> [i128; 4] {
        [
            i128::from(self.cash_delta_cents),
            i128::from(self.position_cost_delta_cents),
            i128::from(self.realized_pnl_expense_cents),
            i128::from(self.fee_delta_cents),
        ]
    }
}
impl std::fmt::Debug for ExecutionDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExecutionDecision(..)")
    }
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        && value.bytes().any(|c| c != b'0')
}
