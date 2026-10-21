//! Disposable account/order/risk read models from authenticated owner exports.
//!
//! No database, broker, clock, or financial mutation capability. Gaps leave the
//! view unchanged; fetch the missing owner revisions and retry, never skip them.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod checkpoint;

use edgeagent_contracts::audit::{AccountAuditFrame, AccountAuditState};
use sha2::{Digest, Sha256};
use std::fmt;

/// Typed, payload-safe replay failure. No state changes on an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReplayError {
    /// Invalid state, financial reconciliation or checkpoint integrity.
    Invalid,
    /// An earlier revision must be fetched first.
    Gap,
    /// Different content at an already retained revision or predecessor.
    Conflict,
    /// Account, run, configuration or risk policy does not match this view.
    Scope,
    /// Explicit non-evicting revision capacity exhausted.
    Capacity,
}
impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "audit replay failed: {self:?}")
    }
}
impl std::error::Error for ReplayError {}

/// Result of applying a complete owner revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayOutcome {
    /// View advanced once.
    Applied,
    /// Identical retained revision; no effects.
    Duplicate,
}

/// Read-only projection with bounded non-evicting duplicate receipts.
pub struct AccountProjection {
    capacity: usize,
    receipts: Vec<String>,
    latest: AccountAuditFrame,
    genesis_cash: u64,
    genesis_cost: Option<u64>,
}
impl fmt::Debug for AccountProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountProjection")
            .field("revision", &self.latest.state().revision)
            .finish_non_exhaustive()
    }
}
impl AccountProjection {
    /// Start only from an authenticated genesis. Capacity includes revision zero.
    ///
    /// # Errors
    /// Reject non-genesis/financial effects or capacity outside 1..=8193.
    pub fn new(genesis: AccountAuditFrame, capacity: usize) -> Result<Self, ReplayError> {
        let s = genesis.state();
        if !(1..=8193).contains(&capacity) {
            return Err(ReplayError::Capacity);
        }
        if s.revision != 0 || s.postings != [0; 4] || !s.orders.is_empty() {
            return Err(ReplayError::Invalid);
        }
        let genesis_cash = s.cash_cents;
        let genesis_cost = cost(s)?;
        let receipt = fingerprint(&genesis)?;
        Ok(Self {
            capacity,
            receipts: vec![receipt],
            latest: genesis,
            genesis_cash,
            genesis_cost,
        })
    }
    /// Apply one contiguous revision; reordered delivery is retried after backfill.
    ///
    /// # Errors
    /// Reject gaps, conflicts, changed scope, capacity or financial drift without
    /// modifying either state or duplicate receipts. Never buffer unbounded gaps.
    pub fn apply(&mut self, frame: AccountAuditFrame) -> Result<ReplayOutcome, ReplayError> {
        let s = frame.state();
        let current = self.latest.state();
        if !same_scope(s, current) {
            return Err(ReplayError::Scope);
        }
        let index = usize::try_from(s.revision).map_err(|_| ReplayError::Invalid)?;
        let digest = fingerprint(&frame)?;
        if let Some(receipt) = self.receipts.get(index) {
            return if receipt == &digest {
                Ok(ReplayOutcome::Duplicate)
            } else {
                Err(ReplayError::Conflict)
            };
        }
        if index != self.receipts.len() {
            return Err(ReplayError::Gap);
        }
        if self.receipts.len() == self.capacity {
            return Err(ReplayError::Capacity);
        }
        if frame.previous() != self.receipts.last().map(String::as_str) {
            return Err(ReplayError::Conflict);
        }
        if s.risk_revision < current.risk_revision
            || s.postings[3] < current.postings[3]
            || i128::from(self.genesis_cash) + i128::from(s.postings[0]) != i128::from(s.cash_cents)
            || current.orders.iter().any(|old| {
                !s.orders.iter().any(|o| {
                    o.order == old.order
                        && o.revision >= old.revision
                        && o.filled >= old.filled
                        && (o.revision != old.revision || o == old)
                        && (!matches!(
                            old.status.as_str(),
                            "filled" | "cancelled" | "expired" | "rejected"
                        ) || o == old)
                        && (&o.strategy, &o.instrument, &o.unit, &o.side, o.quantity)
                            == (
                                &old.strategy,
                                &old.instrument,
                                &old.unit,
                                &old.side,
                                old.quantity,
                            )
                })
            })
        {
            return Err(ReplayError::Invalid);
        }
        if let Some(initial) = self.genesis_cost
            && cost(s)?.map(i128::from) != Some(i128::from(initial) + i128::from(s.postings[1]))
        {
            return Err(ReplayError::Invalid);
        }
        self.receipts.push(digest);
        self.latest = frame;
        Ok(ReplayOutcome::Applied)
    }
    /// Validated latest view. No current/freshness claim beyond its revision.
    #[must_use]
    pub const fn state(&self) -> &AccountAuditState {
        self.latest.state()
    }
    /// Lag against an owner-supplied high-water revision, never wall-clock age.
    ///
    /// # Errors
    /// Reject a high-water mark older than the view (wrong/stale owner snapshot).
    pub fn lag(&self, owner_revision: u64) -> Result<u64, ReplayError> {
        owner_revision
            .checked_sub(self.state().revision)
            .ok_or(ReplayError::Invalid)
    }
}

/// SHA-256 of canonical checked content. Integrity/deduplication, not authenticity.
///
/// # Errors
/// Returns `Invalid` if serialization fails.
pub fn fingerprint(frame: &AccountAuditFrame) -> Result<String, ReplayError> {
    let digest = Sha256::digest(frame.encode().map_err(|_| ReplayError::Invalid)?);
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}
fn cost(s: &AccountAuditState) -> Result<Option<u64>, ReplayError> {
    let mut sum = 0_u64;
    for p in &s.positions {
        let Some(c) = p.cost_cents else {
            return Ok(None);
        };
        sum = sum.checked_add(c).ok_or(ReplayError::Invalid)?;
    }
    Ok(Some(sum))
}
fn same_scope(a: &AccountAuditState, b: &AccountAuditState) -> bool {
    a.account == b.account
        && a.run == b.run
        && a.configuration == b.configuration
        && a.risk_policy == b.risk_policy
        && a.execution_plan == b.execution_plan
        && a.settlement_plan == b.settlement_plan
}
