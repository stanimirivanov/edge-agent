//! Owner-exported simulation audit records, not commands or risk authority.
//!
//! Parse with [`AccountAuditFrame::decode`] before allocating a read model. This
//! contract is an authenticated owner export, not a new CloudEvents message.
use serde::{Deserialize, Serialize};
use std::fmt;

/// Maximum encoded record/checkpoint state size. Exceeding it fails closed.
pub const MAX_AUDIT_BYTES: usize = 1_048_576;

/// Payload-safe audit validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditContractError;
impl fmt::Display for AuditContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid or oversized account audit record")
    }
}
impl std::error::Error for AuditContractError {}

/// Exact retained order projection. Reservations remain held during cancellation.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AuditOrder {
    /// Stable order ID.
    pub order: String,
    /// Owning strategy.
    pub strategy: String,
    /// Stable instrument.
    pub instrument: String,
    /// `shares` or `contracts`.
    pub unit: String,
    /// `buy_to_open` or `sell_to_close`.
    pub side: String,
    /// Journal revision, not account revision.
    pub revision: u64,
    /// Actual lifecycle status, never an `already_terminal` command disposition.
    pub status: String,
    /// Original whole quantity.
    pub quantity: u64,
    /// Cumulative whole quantity filled.
    pub filled: u64,
    /// Remaining USD-cent cash hold.
    pub reserved_cash_cents: u64,
    /// Remaining inventory hold.
    pub reserved_units: u64,
}

/// Inventory with explicit book-cost availability; absent does not mean zero.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AuditPosition {
    /// Strategy allocation.
    pub strategy: String,
    /// Stable instrument.
    pub instrument: String,
    /// `shares` or `contracts`.
    pub unit: String,
    /// Whole long quantity.
    pub quantity: u64,
    /// USD-cent book cost; P08-only accounts may not have provisioned costs.
    #[serde(deserialize_with = "required")]
    pub cost_cents: Option<u64>,
}

/// Risk carry-forward is explicitly not a fresh price or executable quote.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AuditValuation {
    /// Strategy allocation.
    pub strategy: String,
    /// Conservative position gross in USD cents.
    pub gross_cents: u64,
    /// Adverse session contribution in USD cents.
    pub loss_cents: u64,
    /// Original valuation Unix seconds, not audit receipt time.
    pub valued_seconds: i64,
    /// Original nanosecond fraction.
    pub valued_nanos: u32,
    /// Retained original valuation artifact digest.
    pub artifact: String,
}

/// Complete owner-authored revision state. Public fields are construction input;
/// only [`AccountAuditFrame::new`] can admit one to a replay stream.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AccountAuditState {
    /// Account identity.
    pub account: String,
    /// Frozen run digest.
    pub run: String,
    /// Admission configuration digest.
    pub configuration: String,
    /// Optional immutable execution plan binding; absent for admission-only runs.
    #[serde(deserialize_with = "required")]
    pub execution_plan: Option<String>,
    /// Optional immutable final-settlement postlude binding.
    #[serde(deserialize_with = "required")]
    pub settlement_plan: Option<String>,
    /// Contiguous account revision, including genesis zero.
    pub revision: u64,
    /// Canonical command/input position.
    pub input_revision: u64,
    /// Exact USD-cent cash balance.
    pub cash_cents: u64,
    /// Risk-policy content digest.
    pub risk_policy: String,
    /// Authoritative risk revision, independent of account revision.
    pub risk_revision: u64,
    /// Original sticky breach code, including strategy ID for strategy breaches.
    #[serde(deserialize_with = "required")]
    pub risk_breach: Option<String>,
    /// Exact versioned risk continuation, hex encoded, for reconciliation only.
    pub risk_checkpoint: String,
    /// Sorted, unique order views.
    pub orders: Vec<AuditOrder>,
    /// Sorted, unique position views.
    pub positions: Vec<AuditPosition>,
    /// Sorted, unique original valuations plus conservative carry.
    pub valuations: Vec<AuditValuation>,
    /// Cumulative balanced postings: cash, book cost, realized P/L expense, fees.
    pub postings: [i64; 4],
}

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: String,
    #[serde(deserialize_with = "required")]
    previous: Option<String>,
    state: AccountAuditState,
}

/// Checked, bounded export record. Equality compares its entire content.
#[derive(Clone, Serialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct AccountAuditFrame(Frame);
impl fmt::Debug for AuditOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuditOrder").finish_non_exhaustive()
    }
}
impl fmt::Debug for AuditPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuditPosition").finish_non_exhaustive()
    }
}
impl fmt::Debug for AuditValuation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuditValuation").finish_non_exhaustive()
    }
}
impl fmt::Debug for AccountAuditState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountAuditState")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}
impl fmt::Debug for AccountAuditFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountAuditFrame")
            .field("revision", &self.0.state.revision)
            .finish_non_exhaustive()
    }
}
impl AccountAuditFrame {
    /// Validate complete owner state and predecessor digest (none only at zero).
    ///
    /// # Errors
    /// Reject invalid identities, ordering, numeric bounds or oversized state.
    pub fn new(
        state: AccountAuditState,
        previous: Option<String>,
    ) -> Result<Self, AuditContractError> {
        let frame = Self(Frame {
            version: "edgeagent.account-audit.v1".into(),
            previous,
            state,
        });
        frame.validate()?;
        if frame.encode()?.len() > MAX_AUDIT_BYTES {
            return Err(AuditContractError);
        }
        Ok(frame)
    }
    /// Decode bounded bytes; unknown fields/versions and invalid states fail closed.
    ///
    /// # Errors
    /// Returns a payload-safe error without including exported financial data.
    pub fn decode(bytes: &[u8]) -> Result<Self, AuditContractError> {
        if bytes.len() > MAX_AUDIT_BYTES {
            return Err(AuditContractError);
        }
        let frame = Self(serde_json::from_slice(bytes).map_err(|_| AuditContractError)?);
        frame.validate()?;
        Ok(frame)
    }
    /// Canonical field/array ordering used by predecessor digests and deduplication.
    ///
    /// # Errors
    /// Returns a payload-safe serialization failure.
    pub fn encode(&self) -> Result<Vec<u8>, AuditContractError> {
        serde_json::to_vec(self).map_err(|_| AuditContractError)
    }
    /// Read-only validated state; it cannot authorize an account mutation.
    #[must_use]
    pub const fn state(&self) -> &AccountAuditState {
        &self.0.state
    }
    /// Canonical predecessor digest, or none for genesis.
    #[must_use]
    pub fn previous(&self) -> Option<&str> {
        self.0.previous.as_deref()
    }
    fn validate(&self) -> Result<(), AuditContractError> {
        let s = &self.0.state;
        let valid = self.0.version == "edgeagent.account-audit.v1"
            && identity(&s.account, 32)
            && identity(&s.run, 64)
            && identity(&s.configuration, 64)
            && identity(&s.risk_policy, 64)
            && s.revision <= 8192
            && s.input_revision == s.revision
            && s.execution_plan.as_deref().is_none_or(|p| identity(p, 64))
            && s.settlement_plan.as_deref().is_none_or(|p| identity(p, 64))
            && (s.settlement_plan.is_none() || s.execution_plan.is_some())
            && positive(s.risk_revision)
            && amount(s.cash_cents)
            && hex(&s.risk_checkpoint, 316)
            && ((s.revision == 0 && self.previous().is_none())
                || (s.revision > 0 && self.previous().is_some_and(|p| hex(p, 64))))
            && s.orders.len() <= 4096
            && s.positions.len() <= 4096
            && (1..=32).contains(&s.valuations.len())
            && ordered(s.orders.iter().map(|o| o.order.as_str()))
            && ordered(
                s.positions
                    .iter()
                    .map(|p| (&p.strategy, &p.instrument, &p.unit)),
            )
            && ordered(s.valuations.iter().map(|v| v.strategy.as_str()))
            && s.postings.iter().map(|n| i128::from(*n)).sum::<i128>() == 0
            && s.postings[3] >= 0;
        if !valid {
            return Err(AuditContractError);
        }
        if s.risk_breach
            .as_deref()
            .is_some_and(|reason| !breach(reason))
        {
            return Err(AuditContractError);
        }
        let mut cash = 0_u64;
        for o in &s.orders {
            let terminal = matches!(
                o.status.as_str(),
                "filled" | "cancelled" | "expired" | "rejected"
            );
            if !identity(&o.order, 64)
                || !identity(&o.strategy, 32)
                || !identity(&o.instrument, 32)
                || !unit(&o.unit)
                || !matches!(o.side.as_str(), "buy_to_open" | "sell_to_close")
                || !positive(o.revision)
                || o.revision > 4096
                || !positive(o.quantity)
                || o.filled > o.quantity
                || !amount(o.reserved_cash_cents)
                || !amount(o.reserved_units)
                || !matches!(
                    o.status.as_str(),
                    "working"
                        | "partially_filled"
                        | "cancel_pending"
                        | "filled"
                        | "cancelled"
                        | "expired"
                        | "rejected"
                )
                || (terminal && (o.reserved_cash_cents != 0 || o.reserved_units != 0))
                || (o.status == "filled" && o.filled != o.quantity)
                || (o.status == "partially_filled" && (o.filled == 0 || o.filled == o.quantity))
                || (matches!(o.status.as_str(), "working" | "rejected") && o.filled != 0)
                || (o.side == "buy_to_open" && o.reserved_units != 0)
                || o.reserved_units > o.quantity - o.filled
                || !s.valuations.iter().any(|v| v.strategy == o.strategy)
            {
                return Err(AuditContractError);
            }
            cash = cash
                .checked_add(o.reserved_cash_cents)
                .ok_or(AuditContractError)?;
        }
        if cash > s.cash_cents {
            return Err(AuditContractError);
        }
        for p in &s.positions {
            if !identity(&p.strategy, 32)
                || !identity(&p.instrument, 32)
                || !unit(&p.unit)
                || !amount(p.quantity)
                || p.cost_cents
                    .is_some_and(|c| !amount(c) || (p.quantity == 0 && c != 0))
                || !s.valuations.iter().any(|v| v.strategy == p.strategy)
            {
                return Err(AuditContractError);
            }
        }
        for o in s.orders.iter().filter(|o| o.reserved_units > 0) {
            let position = s
                .positions
                .iter()
                .find(|p| {
                    p.strategy == o.strategy && p.instrument == o.instrument && p.unit == o.unit
                })
                .ok_or(AuditContractError)?;
            let held = s
                .orders
                .iter()
                .filter(|other| {
                    other.strategy == o.strategy
                        && other.instrument == o.instrument
                        && other.unit == o.unit
                })
                .try_fold(0_u64, |sum, other| sum.checked_add(other.reserved_units))
                .ok_or(AuditContractError)?;
            if held > position.quantity {
                return Err(AuditContractError);
            }
        }
        for v in &s.valuations {
            if !identity(&v.strategy, 32)
                || !amount(v.gross_cents)
                || !amount(v.loss_cents)
                || v.valued_nanos > 999_999_999
                || !identity(&v.artifact, 64)
            {
                return Err(AuditContractError);
            }
        }
        Ok(())
    }
}
fn amount(n: u64) -> bool {
    n <= i64::MAX as u64
}
fn required<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(d)
}
fn positive(n: u64) -> bool {
    n > 0 && amount(n)
}
fn hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn identity(s: &str, n: usize) -> bool {
    hex(s, n) && s.bytes().any(|b| b != b'0')
}
fn unit(s: &str) -> bool {
    matches!(s, "shares" | "contracts")
}
fn ordered<T: Ord>(items: impl Iterator<Item = T>) -> bool {
    let mut previous = None;
    for item in items {
        if previous.as_ref().is_some_and(|p| p >= &item) {
            return false;
        }
        previous = Some(item);
    }
    true
}
fn breach(reason: &str) -> bool {
    if matches!(
        reason,
        "session_closed" | "stale_valuation" | "account_gross" | "account_loss" | "account_orders"
    ) {
        return true;
    }
    reason.split_once(':').is_some_and(|(code, id)| {
        matches!(code, "strategy_gross" | "strategy_loss" | "strategy_orders") && identity(id, 32)
    })
}
