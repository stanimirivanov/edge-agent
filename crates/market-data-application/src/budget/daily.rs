//! Reviewed fixed UTC-day credit allocation; never a provider entitlement or refund ledger.
use super::{BudgetError, BudgetErrorKind as K, Result};
use edgeagent_market_data_domain::ReferenceId;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

const DAY_MS: i64 = 86_400_000;

/// Immutable owner-reviewed allocation exclusively reserved for this ledger's account.
/// The owner must authenticate the review and fence other consumers of the allocation.
/// A constructor, revision or API key alone proves neither exclusivity nor data rights.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PolicyData")]
pub struct DailyCreditPolicy {
    revision: u64,
    allocation: u32,
    expires_ms: i64,
    costs: BTreeMap<String, u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyData {
    revision: u64,
    allocation: u32,
    expires_ms: i64,
    costs: BTreeMap<String, u32>,
}
impl TryFrom<PolicyData> for DailyCreditPolicy {
    type Error = BudgetError;
    fn try_from(raw: PolicyData) -> Result<Self> {
        let policy = Self {
            revision: raw.revision,
            allocation: raw.allocation,
            expires_ms: raw.expires_ms,
            costs: raw.costs,
        };
        if !policy.valid() {
            return Err(BudgetError::new(K::Invalid));
        }
        Ok(policy)
    }
}
impl fmt::Debug for DailyCreditPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DailyCreditPolicy").finish_non_exhaustive()
    }
}
impl DailyCreditPolicy {
    pub(crate) fn covers_qualification(
        &self,
        endpoint: ReferenceId,
        usage: DailyCreditUsage,
        now: i64,
        until: i64,
        calls: u32,
    ) -> bool {
        let cost = self.costs.get(&format!("{:032x}", endpoint.get()));
        self.valid()
            && calls > 0
            && now < until
            && until <= self.expires_ms
            && usage.revision == self.revision
            && usage.allocation == self.allocation
            && usage.expires_ms == self.expires_ms
            && usage.charged <= self.allocation
            && usage.utc_day == now.div_euclid(DAY_MS)
            && now.div_euclid(DAY_MS) == (until - 1).div_euclid(DAY_MS)
            && cost
                .and_then(|c| c.checked_mul(calls))
                .is_some_and(|needed| needed <= self.allocation - usage.charged)
    }
    /// Review a positive daily allocation, exclusive expiry and 1..=16 endpoint costs.
    /// Costs must conservatively cover the maximum billable request shape, including
    /// symbols/weights; adapters must not dispatch a more expensive shape under that ID.
    /// Only providers with a reviewed 00:00 UTC reset fit this policy version.
    /// # Errors
    /// Rejects zero/duplicate identities, impossible prices, expiry or oversized reviews.
    pub fn new(
        revision: u64,
        allocation: u32,
        expires_ms: i64,
        costs: &[(ReferenceId, u32)],
    ) -> Result<Self> {
        if !(1..=16).contains(&costs.len()) {
            return Err(BudgetError::new(K::Invalid));
        }
        let mut prices = BTreeMap::new();
        for (endpoint, price) in costs {
            if prices
                .insert(format!("{:032x}", endpoint.get()), *price)
                .is_some()
            {
                return Err(BudgetError::new(K::Invalid));
            }
        }
        let policy = Self {
            revision,
            allocation,
            expires_ms,
            costs: prices,
        };
        if !policy.valid() {
            return Err(BudgetError::new(K::Invalid));
        }
        Ok(policy)
    }
    fn valid(&self) -> bool {
        self.revision > 0
            && self.allocation > 0
            && self.expires_ms > 0
            && (1..=16).contains(&self.costs.len())
            && self.costs.iter().all(|(id, cost)| {
                id.len() == 32
                    && u128::from_str_radix(id, 16)
                        .is_ok_and(|n| n > 0 && format!("{n:032x}") == *id)
                    && *cost > 0
                    && *cost <= self.allocation
            })
    }
}

/// Persisted usage, not a dispatch grant. `utc_day` describes the last committed
/// accounting transition; a read never advances a window or releases a fence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DailyCreditUsage {
    /// Authenticated composition must resolve this owner-reviewed policy revision.
    pub revision: u64,
    /// UTC days since Unix epoch, not an exchange session or rolling 24 hours.
    pub utc_day: i64,
    /// Conservative charged credits, including initialization and uncertain calls.
    pub charged: u32,
    /// Exclusive account-wide allocation, not a provider's advertised total quota.
    pub allocation: u32,
    /// Exclusive review expiry in Unix milliseconds.
    pub expires_ms: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DailyCredits {
    policy: DailyCreditPolicy,
    opening_charge: u32,
    high_water_ms: i64,
    charged: u32,
}
impl DailyCredits {
    pub(super) fn new(policy: DailyCreditPolicy, opening_charge: u32, now: i64) -> Result<Self> {
        let state = Self {
            policy,
            opening_charge,
            high_water_ms: now,
            charged: opening_charge,
        };
        if !state.valid() || now >= state.policy.expires_ms {
            return Err(BudgetError::new(K::Invalid));
        }
        Ok(state)
    }
    pub(super) fn valid(&self) -> bool {
        self.policy.valid()
            && self.high_water_ms >= 0
            && self.opening_charge <= self.policy.allocation
            && self.charged <= self.policy.allocation
    }
    pub(super) fn matches(&self, policy: &DailyCreditPolicy, opening: u32) -> bool {
        &self.policy == policy && self.opening_charge == opening
    }
    pub(super) fn usage(&self) -> DailyCreditUsage {
        DailyCreditUsage {
            revision: self.policy.revision,
            utc_day: self.high_water_ms.div_euclid(DAY_MS),
            charged: self.charged,
            allocation: self.policy.allocation,
            expires_ms: self.policy.expires_ms,
        }
    }
    // A delayed remote debit can land in the completion day. Charge both plausible
    // days, never refund the reservation day, and preserve clock high-water fencing.
    pub(super) fn complete(&mut self, endpoint: ReferenceId, now: i64) -> Result<()> {
        let price = self
            .policy
            .costs
            .get(&format!("{:032x}", endpoint.get()))
            .ok_or_else(|| BudgetError::new(K::Invalid))?;
        if now.div_euclid(DAY_MS) > self.high_water_ms.div_euclid(DAY_MS) {
            self.charged = *price;
        }
        self.high_water_ms = self.high_water_ms.max(now);
        Ok(())
    }
    pub(super) fn confirm_retired(&mut self, now: i64) {
        // Retirement erased endpoint identity. Close the confirmation-day allocation
        // rather than guessing its price or remote debit time, even on the same day.
        self.charged = self.policy.allocation;
        self.high_water_ms = self.high_water_ms.max(now);
    }
    // Invoke only after ALL command/rate/fence checks. Failures never partially mutate state.
    pub(super) fn charge(&mut self, endpoint: ReferenceId, now: i64) -> Result<()> {
        if now < self.high_water_ms {
            return Err(BudgetError::new(K::Invalid));
        }
        if now >= self.policy.expires_ms {
            return Err(BudgetError::new(K::Denied));
        }
        let price = self
            .policy
            .costs
            .get(&format!("{:032x}", endpoint.get()))
            .ok_or_else(|| BudgetError::new(K::Denied))?;
        let prior = if now.div_euclid(DAY_MS) == self.high_water_ms.div_euclid(DAY_MS) {
            self.charged
        } else {
            0
        };
        let charged = prior
            .checked_add(*price)
            .filter(|n| *n <= self.policy.allocation)
            .ok_or_else(|| BudgetError::new(K::Exhausted))?;
        self.charged = charged;
        self.high_water_ms = now;
        Ok(())
    }
}
