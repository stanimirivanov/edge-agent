//! Synthetic dry-run admission v1. No caller-provided price, cash amount or actor.

use crate::{Component, EventRetention, MessageDefinition, MessageRegistry, MessageRoutingError};
use serde::{Deserialize, Serialize};

/// Command followed by its authoritative simulator outcome.
pub const DEFINITIONS: [MessageDefinition; 2] = [
    MessageDefinition::command(
        "com.edgeagent.simulation.submit-order.v1",
        "urn:edgeagent:schema:simulation:submit-order:v1",
        Component::ExecutionSimulator,
        "account",
    ),
    MessageDefinition::event(
        "com.edgeagent.simulation.order-admission-decided.v1",
        "urn:edgeagent:schema:simulation:order-admission:v1",
        Component::ExecutionSimulator,
        "account",
        EventRetention::Audit,
    ),
];
/// Validated production registry; provisioning remains composition-owned.
pub fn registry() -> Result<MessageRegistry<'static>, MessageRoutingError> {
    MessageRegistry::new(&DEFINITIONS)
}
/// Immutable v1 JSON Schema, resolved locally, never fetched by schema URI.
pub const SUBMIT_SCHEMA: &str = include_str!("../schemas/simulation/submit-order-v1.json");
/// Immutable v1 JSON Schema for committed admission outcomes.
pub const OUTCOME_SCHEMA: &str = include_str!("../schemas/simulation/order-admission-v1.json");

/// Payload-free wire validation error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdmissionContractError;
impl std::fmt::Display for AdmissionContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid dry-run admission contract")
    }
}
impl std::error::Error for AdmissionContractError {}

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
struct SubmitPayload {
    mode: String,
    account: String,
    order: String,
    quote: String,
    expected_revision: u64,
    input_revision: u64,
}
/// Intent referencing a service-owned immutable synthetic quote. Actor, price,
/// funding and canonical time cannot be supplied through this payload.
#[derive(Clone, Serialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct SubmitDryRunOrder(SubmitPayload);
impl<'de> Deserialize<'de> for SubmitDryRunOrder {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        SubmitPayload::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}
impl TryFrom<SubmitPayload> for SubmitDryRunOrder {
    type Error = AdmissionContractError;
    fn try_from(value: SubmitPayload) -> Result<Self, Self::Error> {
        Self::new(
            &value.mode,
            &value.account,
            &value.order,
            &value.quote,
            value.expected_revision,
            value.input_revision,
        )
    }
}
impl SubmitDryRunOrder {
    /// Validate exact mode, fixed lowercase nonzero hex IDs and bounded revisions.
    pub fn new(
        mode: &str,
        account: &str,
        order: &str,
        quote: &str,
        expected_revision: u64,
        input_revision: u64,
    ) -> Result<Self, AdmissionContractError> {
        if mode != "dry_run"
            || !hex(account, 32)
            || !hex(order, 64)
            || !hex(quote, 64)
            || expected_revision >= i64::MAX as u64
            || input_revision == 0
            || input_revision > i64::MAX as u64
        {
            return Err(AdmissionContractError);
        }
        Ok(Self(SubmitPayload {
            mode: mode.into(),
            account: account.into(),
            order: order.into(),
            quote: quote.into(),
            expected_revision,
            input_revision,
        }))
    }
    /// Account, order and immutable synthetic quote fingerprint.
    #[must_use]
    pub fn identity(&self) -> (&str, &str, &str) {
        (&self.0.account, &self.0.order, &self.0.quote)
    }
    /// Expected account revision and next canonical account input revision.
    #[must_use]
    pub const fn position(&self) -> (u64, u64) {
        (self.0.expected_revision, self.0.input_revision)
    }
}
impl std::fmt::Debug for SubmitDryRunOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubmitDryRunOrder").finish_non_exhaustive()
    }
}
/// Stable admission decision; rejection records consume the canonical input too.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AdmissionStatus {
    /// Cash/inventory and risk were reserved atomically.
    Admitted,
    /// Current unreserved account cash cannot fund the quote.
    InsufficientCash,
    /// Current unreserved long inventory cannot fund a sell.
    InsufficientInventory,
    /// P06 policy denied the aggregate/strategy increase or retained a breach.
    RiskDenied,
}
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
struct OutcomePayload {
    request: SubmitPayload,
    run: String,
    strategy: String,
    revision: u64,
    order_revision: u8,
    status: AdmissionStatus,
    cash_reserved_cents: u64,
    inventory_reserved: u64,
    risk_revision: u64,
    risk_reason: Option<String>,
}
/// Immutable committed admission fact; not a fill or permission for live routing.
#[derive(Clone, Serialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct OrderAdmission(OutcomePayload);
impl<'de> Deserialize<'de> for OrderAdmission {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        OutcomePayload::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}
impl TryFrom<OutcomePayload> for OrderAdmission {
    type Error = AdmissionContractError;
    fn try_from(value: OutcomePayload) -> Result<Self, Self::Error> {
        let request = SubmitDryRunOrder::try_from(value.request.clone())?;
        let outcome = Self::new(
            &request,
            &value.run,
            &value.strategy,
            value.revision,
            value.status,
            (value.cash_reserved_cents, value.inventory_reserved),
            (value.risk_revision, value.risk_reason.as_deref()),
        )?;
        if value.order_revision != 1 {
            return Err(AdmissionContractError);
        }
        Ok(outcome)
    }
}
impl OrderAdmission {
    /// Build a revisioned fact. Amounts are computed by the owner, not the command.
    pub fn new(
        request: &SubmitDryRunOrder,
        run: &str,
        strategy: &str,
        revision: u64,
        status: AdmissionStatus,
        reserved: (u64, u64),
        risk: (u64, Option<&str>),
    ) -> Result<Self, AdmissionContractError> {
        if !hex(run, 64)
            || !hex(strategy, 32)
            || revision != request.0.expected_revision + 1
            || risk.0 == 0
            || risk.0 > i64::MAX as u64
            || (status != AdmissionStatus::Admitted && reserved != (0, 0))
            || (status == AdmissionStatus::RiskDenied) != risk.1.is_some()
            || risk.1.is_some_and(|reason| {
                !matches!(
                    reason,
                    "session_closed"
                        | "stale_valuation"
                        | "account_gross"
                        | "account_loss"
                        | "account_orders"
                        | "strategy_gross"
                        | "strategy_loss"
                        | "strategy_orders"
                )
            })
        {
            return Err(AdmissionContractError);
        }
        Ok(Self(OutcomePayload {
            request: request.0.clone(),
            run: run.into(),
            strategy: strategy.into(),
            revision,
            order_revision: 1,
            status,
            cash_reserved_cents: reserved.0,
            inventory_reserved: reserved.1,
            risk_revision: risk.0,
            risk_reason: risk.1.map(str::to_owned),
        }))
    }
    /// Original immutable request, including its original optimistic preconditions.
    #[must_use]
    pub fn request(&self) -> SubmitDryRunOrder {
        SubmitDryRunOrder(self.0.request.clone())
    }
    /// Resulting account revision and decision.
    #[must_use]
    pub const fn decision(&self) -> (u64, AdmissionStatus) {
        (self.0.revision, self.0.status)
    }
    /// Cash cents and whole long inventory reserved by this decision.
    #[must_use]
    pub const fn reservation(&self) -> (u64, u64) {
        (self.0.cash_reserved_cents, self.0.inventory_reserved)
    }
}
impl std::fmt::Debug for OrderAdmission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrderAdmission")
            .field("status", &self.0.status)
            .finish_non_exhaustive()
    }
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}
