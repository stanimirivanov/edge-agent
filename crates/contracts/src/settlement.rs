//! Identity-only settlement intent and bounded, balanced committed account facts.
use crate::execution::{ExecuteDryRunStep, ExecutionContractError};
use crate::{Component, EventRetention, MessageDefinition, MessageRegistry, MessageRoutingError};
use serde::{Deserialize, Serialize};

/// Dedicated settlement routes; existing execution/admission v1 semantics do not change.
pub const DEFINITIONS: [MessageDefinition; 2] = [
    MessageDefinition::command(
        "com.edgeagent.simulation.settle-account.v1",
        "urn:edgeagent:schema:simulation:settle-account:v1",
        Component::ExecutionSimulator,
        "account",
    ),
    MessageDefinition::event(
        "com.edgeagent.simulation.settlement-decided.v1",
        "urn:edgeagent:schema:simulation:settlement-decided:v1",
        Component::ExecutionSimulator,
        "account",
        EventRetention::Audit,
    ),
];
/// Locally resolved immutable command schema.
pub const COMMAND_SCHEMA: &str = include_str!("../schemas/simulation/settle-account-v1.json");
/// Locally resolved immutable decision schema.
pub const DECISION_SCHEMA: &str = include_str!("../schemas/simulation/settlement-decided-v1.json");
/// Settlement's dedicated registry, unioned by composition when required.
pub fn registry() -> Result<MessageRegistry<'static>, MessageRoutingError> {
    MessageRegistry::new(&DEFINITIONS)
}

/// Plan/slot identity, with no caller-supplied money, time, fixing or actor.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct SettleDryRunAccount(ExecuteDryRunStep);
impl SettleDryRunAccount {
    /// Require exact dry-run mode and checked account/plan/revision identifiers.
    /// # Errors
    /// Reject unsupported mode, malformed identities or SQL-sized revision overflow.
    pub fn new(
        mode: &str,
        account: &str,
        plan: &str,
        sequence: u64,
        expected: u64,
    ) -> Result<Self, ExecutionContractError> {
        ExecuteDryRunStep::new(mode, account, plan, sequence, expected).map(Self)
    }
    /// Account, immutable settlement plan, canonical slot and expected account revision.
    #[must_use]
    pub fn position(&self) -> (&str, &str, u64, u64) {
        self.0.position()
    }
}
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct AllocationPayload {
    strategy: String,
    contracts: u64,
    gross_cents: u64,
    fee_cents: u64,
    postings: [i64; 4],
}
/// One strategy attribution; rounding occurred once at account/instrument level.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(try_from = "AllocationPayload", into = "AllocationPayload")]
pub struct SettlementAllocation(AllocationPayload);
impl SettlementAllocation {
    /// Validate positive quantity, SQL-sized values and P09-compatible balanced postings.
    /// # Errors
    /// Reject invalid strategy, negative cost-release convention or unbalanced amounts.
    pub fn new(
        strategy: &str,
        contracts: u64,
        gross: u64,
        fees: u64,
        postings: [i64; 4],
    ) -> Result<Self, ExecutionContractError> {
        if !hex(strategy, 32)
            || contracts == 0
            || [contracts, gross, fees]
                .iter()
                .any(|n| *n > i64::MAX as u64)
            || postings[1] > 0
            || i128::from(postings[0]) != i128::from(gross) - i128::from(fees)
            || i128::from(postings[3]) != i128::from(fees)
            || postings.iter().map(|n| i128::from(*n)).sum::<i128>() != 0
        {
            return Err(ExecutionContractError);
        }
        Ok(Self(AllocationPayload {
            strategy: strategy.into(),
            contracts,
            gross_cents: gross,
            fee_cents: fees,
            postings,
        }))
    }
    /// Strategy, whole contracts, gross/fee cents and signed cash/cost/realized/fee postings.
    #[must_use]
    pub fn parts(&self) -> (&str, u64, u64, u64, [i64; 4]) {
        (
            &self.0.strategy,
            self.0.contracts,
            self.0.gross_cents,
            self.0.fee_cents,
            self.0.postings,
        )
    }
}
impl TryFrom<AllocationPayload> for SettlementAllocation {
    type Error = ExecutionContractError;
    fn try_from(p: AllocationPayload) -> Result<Self, Self::Error> {
        Self::new(
            &p.strategy,
            p.contracts,
            p.gross_cents,
            p.fee_cents,
            p.postings,
        )
    }
}
impl From<SettlementAllocation> for AllocationPayload {
    fn from(p: SettlementAllocation) -> Self {
        p.0
    }
}

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct FactPayload {
    request: SettleDryRunAccount,
    account_revision: u64,
    instrument: String,
    status: String,
    run: String,
    policy: String,
    #[serde(deserialize_with = "required_evidence")]
    evidence: Option<String>,
    allocations: Vec<SettlementAllocation>,
    expired_orders: Vec<(String, u64)>,
}
/// Retained decision, including deferrals and already-settled zero-effect completions.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(try_from = "FactPayload", into = "FactPayload")]
pub struct AccountSettlement(FactPayload);
fn required_evidence<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}
impl AccountSettlement {
    /// Build a bounded committed fact. Allocations and expired orders must be unique
    /// and sorted by identity. Deferred/already-settled decisions have no effects.
    /// # Errors
    /// Reject unsupported statuses, identity/ordering errors or aggregate SQL overflow.
    pub fn new(
        request: SettleDryRunAccount,
        instrument: &str,
        status: &str,
        provenance: (&str, &str, Option<&str>),
        allocations: Vec<SettlementAllocation>,
        expired_orders: Vec<(String, u64)>,
    ) -> Result<Self, ExecutionContractError> {
        let (run, policy, evidence) = provenance;
        let settled = status == "settled";
        if !hex(instrument, 32)
            || !hex(run, 64)
            || !hex(policy, 64)
            || evidence.is_some_and(|e| !hex(e, 64))
            || !matches!(
                status,
                "settled"
                    | "already_settled"
                    | "deferred_not_due"
                    | "deferred_missing"
                    | "deferred_not_known"
            )
            || matches!(status, "settled" | "already_settled") != evidence.is_some()
            || allocations.len() > 32
            || expired_orders.len() > 32
            || (!settled && (!allocations.is_empty() || !expired_orders.is_empty()))
            || allocations
                .windows(2)
                .any(|p| matches!(p,[a,b] if a.parts().0>=b.parts().0))
            || expired_orders
                .windows(2)
                .any(|p| matches!(p,[a,b] if a.0>=b.0))
            || expired_orders
                .iter()
                .any(|(id, rev)| !hex(id, 64) || *rev == 0 || *rev > i64::MAX as u64)
        {
            return Err(ExecutionContractError);
        }
        let mut totals = [0_i128; 7];
        for a in &allocations {
            let (_, q, g, f, p) = a.parts();
            for (total, value) in totals.iter_mut().zip([
                i128::from(q),
                i128::from(g),
                i128::from(f),
                i128::from(p[0]),
                i128::from(p[1]),
                i128::from(p[2]),
                i128::from(p[3]),
            ]) {
                *total += value;
            }
        }
        if totals.iter().any(|n| i64::try_from(*n).is_err()) {
            return Err(ExecutionContractError);
        }
        let account_revision = request.position().3 + 1;
        Ok(Self(FactPayload {
            request,
            account_revision,
            instrument: instrument.into(),
            status: status.into(),
            run: run.into(),
            policy: policy.into(),
            evidence: evidence.map(str::to_owned),
            allocations,
            expired_orders,
        }))
    }
    /// Original command, committed account revision and decision status.
    #[must_use]
    pub fn position(&self) -> (&SettleDryRunAccount, u64, &str) {
        (&self.0.request, self.0.account_revision, &self.0.status)
    }
    /// Deterministic strategy allocations and order expiry revisions.
    #[must_use]
    pub fn effects(&self) -> (&[SettlementAllocation], &[(String, u64)]) {
        (&self.0.allocations, &self.0.expired_orders)
    }
}
impl TryFrom<FactPayload> for AccountSettlement {
    type Error = ExecutionContractError;
    fn try_from(p: FactPayload) -> Result<Self, Self::Error> {
        let expected = p.account_revision;
        let fact = Self::new(
            p.request,
            &p.instrument,
            &p.status,
            (&p.run, &p.policy, p.evidence.as_deref()),
            p.allocations,
            p.expired_orders,
        )?;
        if fact.0.account_revision != expected {
            return Err(ExecutionContractError);
        }
        Ok(fact)
    }
}
impl From<AccountSettlement> for FactPayload {
    fn from(p: AccountSettlement) -> Self {
        p.0
    }
}
macro_rules! redacted {($($t:ty),+)=>{$(impl std::fmt::Debug for $t {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(concat!(stringify!($t),"(..)"))}})+};}
redacted!(SettleDryRunAccount, SettlementAllocation, AccountSettlement);
fn hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        && s.bytes().any(|c| c != b'0')
}
