//! Durable accounting transitions, independent of PostgreSQL and provider SDKs.
use crate::AdmissionRecord;
use edgeagent_contracts::evidence::{AcquireEvidence, EvidenceId};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

mod daily;
mod error;
mod port;
use daily::DailyCredits;
pub use daily::{DailyCreditPolicy, DailyCreditUsage};
pub use error::{BudgetError, BudgetErrorKind};
pub use port::AcquisitionLedger;

type Result<T> = std::result::Result<T, BudgetError>;
use BudgetErrorKind as K;

/// Monotonic per-command call identity, not a provider credential or authority grant.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PermitData")]
pub struct CallPermit {
    command: AcquireEvidence,
    ordinal: u32,
    reserved_bytes: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PermitData {
    command: AcquireEvidence,
    ordinal: u32,
    reserved_bytes: u32,
}
impl TryFrom<PermitData> for CallPermit {
    type Error = BudgetError;
    fn try_from(data: PermitData) -> Result<Self> {
        let permit = Self {
            command: data.command,
            ordinal: data.ordinal,
            reserved_bytes: data.reserved_bytes,
        };
        if !valid_permit(&permit) {
            return Err(BudgetError::new(K::Invalid));
        }
        Ok(permit)
    }
}
impl fmt::Debug for CallPermit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallPermit")
            .field("ordinal", &self.ordinal)
            .finish_non_exhaustive()
    }
}
/// A local request has finished or the owner confirmed its worker/request is quiescent.
/// Timeout/cancellation alone is NOT evidence for `QuiescedUnknown`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CallCompletion {
    /// Actual decoded bytes include partial/error responses; honor an explicit provider delay.
    Observed {
        /// Exact bytes, bounded by the pre-I/O reservation.
        decoded_bytes: u32,
        /// Provider-directed delay; never a retry authorization.
        #[serde(deserialize_with = "required")]
        retry_after_ms: Option<u32>,
    },
    /// Owner-confirmed local quiescence with unknown remote outcome; retain the full charge.
    QuiescedUnknown,
}
/// Observable persisted command counters; no prices, credentials or raw admission bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetSnapshot {
    /// Started acquisition attempts, including interrupted ones.
    pub attempts: u8,
    /// Charged external call starts, including failures and uncertain outcomes.
    pub calls: u32,
    /// Observed bytes plus conservative reservations/unknown-outcome charges.
    pub charged_bytes: u32,
    /// Durable cancellation; never implies rollback of an external call.
    pub cancelled: bool,
    /// Unresolved call fence; restart must reconcile it before new dispatch.
    pub pending: Option<CallPermit>,
}
// Keep unchecked deserialization private; callers can only recover validated state.
macro_rules! state { ($name:ident,$raw:ident,$doc:literal,{$($field:ident:$ty:ty),*}) => {
    #[doc=$doc] #[derive(Clone,Serialize)] pub struct $name { $($field:$ty),* }
    #[derive(Deserialize)] #[serde(deny_unknown_fields)] struct $raw { $(#[serde(deserialize_with="required")] $field:$ty),* }
    impl From<$raw> for $name {fn from(raw:$raw)->Self {Self {$($field:raw.$field),*}}}
};}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> std::result::Result<T, D::Error> {
    T::deserialize(decoder)
}
/// Service-owned account accounting, locked before command state.
#[derive(Clone, Serialize)]
pub struct AccountBudget {
    version: u8,
    spacing_ms: u32,
    starts_per_minute: u8,
    starts: Vec<i64>,
    pending: Option<CallPermit>,
    not_before: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    daily: Option<DailyCredits>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountData {
    version: u8,
    spacing_ms: u32,
    starts_per_minute: u8,
    starts: Vec<i64>,
    #[serde(deserialize_with = "required")]
    pending: Option<CallPermit>,
    not_before: i64,
    #[serde(default)]
    daily: Option<DailyCredits>,
}
impl From<AccountData> for AccountBudget {
    fn from(raw: AccountData) -> Self {
        Self {
            version: raw.version,
            spacing_ms: raw.spacing_ms,
            starts_per_minute: raw.starts_per_minute,
            starts: raw.starts,
            pending: raw.pending,
            not_before: raw.not_before,
            daily: raw.daily,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClosedCall {
    permit: CallPermit,
    outcome: CallCompletion,
}
state!(AcquisitionBudget, AcquisitionData, "Service-owned command counters; immutable limits stay in admission.", {
    version:u8, attempts:u8, calls:u32, bytes:u32, cancelled:bool, pending:Option<CallPermit>, pages:BTreeMap<String,u32>, retry_exhausted:bool, completed:Vec<ClosedCall>
});
macro_rules! codec {
    ($ty:ty, $raw:ty, $validate:expr) => {
        impl $ty {
            /// Encode bounded private accounting state; not a public event format.
            /// # Errors
            /// Rejects an oversized or inconsistent state.
            pub fn encode(&self) -> Result<Vec<u8>> {
                self.validate()?;
                serde_json::to_vec(self).map_err(|_| BudgetError::new(K::Invalid))
            }
            /// Decode bounded state and validate invariants before use.
            /// # Errors
            /// Rejects malformed, unsupported or inconsistent private state.
            pub fn decode(bytes: &[u8]) -> Result<Self> {
                if bytes.len() > 8192 {
                    return Err(BudgetError::new(K::Invalid));
                }
                let raw: $raw =
                    serde_json::from_slice(bytes).map_err(|_| BudgetError::new(K::Invalid))?;
                let state = Self::from(raw);
                state.validate()?;
                Ok(state)
            }
            fn validate(&self) -> Result<()> {
                if ($validate)(self) {
                    Ok(())
                } else {
                    Err(BudgetError::new(K::Invalid))
                }
            }
        }
    };
}
fn valid_permit(p: &CallPermit) -> bool {
    (1..=12).contains(&p.ordinal) && (1..=1_048_576).contains(&p.reserved_bytes)
}
codec!(AccountBudget, AccountData, |s: &AccountBudget| ((s
    .version
    == 1
    && s.daily.is_none())
    || (s.version == 2
        && s.daily.as_ref().is_some_and(DailyCredits::valid)))
    && (15000..=300000).contains(&s.spacing_ms)
    && (1..=4).contains(&s.starts_per_minute)
    && s.starts.len() <= 4
    && s.starts
        .windows(2)
        .all(|w| matches!(w, [left, right] if left <= right))
    && s.pending.as_ref().is_none_or(valid_permit));
codec!(
    AcquisitionBudget,
    AcquisitionData,
    |s: &AcquisitionBudget| s.version == 1
        && s.attempts <= 3
        && s.calls <= 12
        && s.bytes <= 8_388_608
        && s.pages.len() <= 12
        && s.pages
            .iter()
            .all(|(k, v)| EvidenceId::new(k).is_ok() && (1..=4).contains(v))
        && s.pages.values().sum::<u32>() <= s.calls
        && s.pending
            .as_ref()
            .is_none_or(|p| valid_permit(p) && p.ordinal == s.calls && p.reserved_bytes <= s.bytes)
        && (s.attempts > 0
            || (s.calls == 0 && s.bytes == 0 && s.pages.is_empty() && s.pending.is_none()))
        && valid_receipts(s)
);
fn valid_receipts(s: &AcquisitionBudget) -> bool {
    if s.completed.len() > 12
        || s.completed.len() + usize::from(s.pending.is_some()) != s.calls as usize
    {
        return false;
    }
    let mut total = s.pending.as_ref().map_or(0, |p| p.reserved_bytes);
    for (index, closed) in s.completed.iter().enumerate() {
        if !valid_permit(&closed.permit) || closed.permit.ordinal as usize != index + 1 {
            return false;
        }
        total += match closed.outcome {
            CallCompletion::Observed { decoded_bytes, .. }
                if decoded_bytes <= closed.permit.reserved_bytes =>
            {
                decoded_bytes
            }
            CallCompletion::QuiescedUnknown => closed.permit.reserved_bytes,
            _ => return false,
        };
    }
    total == s.bytes
}
impl AccountBudget {
    /// Erase a retiring command's content fingerprint without releasing its safety requirement.
    /// The caller MUST persist an opaque account block atomically when this returns true.
    #[must_use = "Persist an opaque account block atomically when a pending permit was erased."]
    pub fn retire_pending(&mut self, request: &EvidenceId) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| &p.command.request == request)
        {
            self.pending = None;
            true
        } else {
            false
        }
    }
    /// Create an explicitly limited account; no permissive provider defaults.
    /// # Errors
    /// Rejects limits outside the supported local ceilings.
    pub fn new(limits: crate::AcquisitionLimits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            version: 1,
            spacing_ms: limits.spacing_ms,
            starts_per_minute: limits.starts_per_minute,
            starts: Vec::new(),
            pending: None,
            not_before: i64::MIN,
            daily: None,
        })
    }
    /// Install an immutable reviewed allocation after owner-confirmed quiescence.
    /// `already_charged` MUST cover all consumption today before this installation,
    /// including other consumers. Unknown usage/exclusivity forbids installation.
    /// Identical retries retain all charges; policy changes/resets are unsupported.
    /// # Errors
    /// Denies pending work, conflicting reviews, invalid time or understated known starts.
    pub fn configure_daily_credits(
        &mut self,
        policy: DailyCreditPolicy,
        already_charged: u32,
        now: i64,
    ) -> Result<()> {
        self.validate()?;
        if let Some(daily) = &self.daily {
            return if daily.matches(&policy, already_charged) {
                Ok(())
            } else {
                Err(BudgetError::new(K::Conflict))
            };
        }
        if self.pending.is_some() {
            return Err(BudgetError::new(K::Busy));
        }
        let known = self
            .starts
            .iter()
            .filter(|at| at.div_euclid(86_400_000) == now.div_euclid(86_400_000))
            .count();
        if self.starts.last().is_some_and(|at| *at > now)
            || u64::from(already_charged) < known as u64
        {
            return Err(BudgetError::new(K::Invalid));
        }
        let daily = DailyCredits::new(policy, already_charged, now)?;
        self.daily = Some(daily);
        self.version = 2;
        Ok(())
    }
    /// Persisted redacted usage; absence denies external dispatch, never means unlimited.
    #[must_use]
    pub fn daily_usage(&self) -> Option<DailyCreditUsage> {
        self.daily.as_ref().map(DailyCredits::usage)
    }
    /// Whether the complete installed policy matches; revision alone cannot establish prices.
    #[must_use]
    pub fn matches_daily_policy(&self, policy: &DailyCreditPolicy) -> bool {
        self.daily
            .as_ref()
            .is_some_and(|daily| daily.matches_policy(policy))
    }
    /// Account-wide unresolved call, including other commands; never permission to clear it.
    #[must_use]
    pub const fn has_pending_call(&self) -> bool {
        self.pending.is_some()
    }
    /// Preserve conservative credits when confirming an opaque retired-call block.
    /// Caller MUST atomically check the exact blocked generation and owner quiescence.
    /// Closes the confirmation-day allocation because retirement erased the price.
    pub fn confirm_retired_daily_credits(&mut self, now: i64) {
        if let Some(daily) = &mut self.daily {
            daily.confirm_retired(now);
        }
    }
    /// New admissions may tighten shared limits, never relax previously reviewed limits.
    /// # Errors
    /// Rejects unreviewed values outside local ceilings before mutation.
    pub fn tighten(&mut self, limits: crate::AcquisitionLimits) -> Result<()> {
        limits.validate()?;
        self.spacing_ms = self.spacing_ms.max(limits.spacing_ms);
        self.starts_per_minute = self.starts_per_minute.min(limits.starts_per_minute);
        Ok(())
    }
}
impl Default for AcquisitionBudget {
    fn default() -> Self {
        Self {
            version: 1,
            attempts: 0,
            calls: 0,
            bytes: 0,
            cancelled: false,
            pending: None,
            pages: BTreeMap::new(),
            retry_exhausted: false,
            completed: Vec::new(),
        }
    }
}
impl AcquisitionBudget {
    /// Validate recovered counters and call fences against their immutable admission.
    /// # Errors
    /// Rejects foreign command receipts or counters exceeding the retained lower limits.
    pub fn verify(&self, r: &AdmissionRecord) -> Result<()> {
        self.validate()?;
        let budget = r.request().budget;
        let bound = |p: &CallPermit| {
            &p.command == r.command() && p.reserved_bytes == budget.response_bytes.get()
        };
        if self.calls > budget.calls.get()
            || self.bytes > budget.command_bytes.get()
            || self.attempts > r.limits().attempts
            || self.pages.values().any(|n| *n > budget.pages.get())
            || self.pending.as_ref().is_some_and(|p| !bound(p))
            || self.completed.iter().any(|c| !bound(&c.permit))
        {
            return Err(BudgetError::new(K::Invalid));
        }
        Ok(())
    }
    /// Inspect durable counters without granting a new call.
    #[must_use]
    pub fn snapshot(&self) -> BudgetSnapshot {
        BudgetSnapshot {
            attempts: self.attempts,
            calls: self.calls,
            charged_bytes: self.bytes,
            cancelled: self.cancelled,
            pending: self.pending.clone(),
        }
    }
    fn ready(&self, r: &AdmissionRecord, now: i64) -> Result<()> {
        self.verify(r)?;
        if self.cancelled {
            return Err(BudgetError::new(K::Cancelled));
        }
        if now < r.request().admitted_at.unix_millis() {
            return Err(BudgetError::new(K::Invalid));
        }
        if now >= r.valid_until_ms()? {
            return Err(BudgetError::new(K::Deadline));
        }
        if self.retry_exhausted {
            return Err(BudgetError::new(K::Exhausted));
        }
        Ok(())
    }
    /// Persist an attempt charge before external work; no counter resets on restart.
    /// # Errors
    /// Rejects cancellation, expiry, unresolved calls or an exhausted attempt allowance.
    pub fn begin_attempt(&mut self, r: &AdmissionRecord, attempt: u8, now: i64) -> Result<()> {
        self.ready(r, now)?;
        if attempt == self.attempts && attempt > 0 {
            return Ok(());
        }
        if self.pending.is_some() {
            return Err(BudgetError::new(K::Busy));
        }
        if attempt > r.limits().attempts {
            return Err(BudgetError::new(K::Exhausted));
        }
        if attempt != self.attempts + 1 {
            return Err(BudgetError::new(K::Conflict));
        }
        self.attempts = attempt;
        self.pages.clear();
        Ok(())
    }
    /// Atomically reserve an account slot, one call and its maximum response bytes.
    /// `traversal` is present for a page and absent for a reference call; all calls count.
    /// # Errors
    /// Rejects unresolved work, rate limits or exhausted command/traversal ceilings.
    pub fn start(
        &mut self,
        account: &mut AccountBudget,
        r: &AdmissionRecord,
        traversal: Option<&EvidenceId>,
        now: i64,
    ) -> Result<CallPermit> {
        self.ready(r, now)?;
        account.validate()?;
        if self.pending.is_some() || account.pending.is_some() {
            return Err(BudgetError::new(K::Busy));
        }
        if self.attempts == 0 {
            return Err(BudgetError::new(K::Invalid));
        }
        let b = r.request().budget;
        let page = traversal.map(|id| id.as_str().to_owned());
        if self.calls >= b.calls.get()
            || b.response_bytes.get() > b.command_bytes.get().saturating_sub(self.bytes)
            || page
                .as_ref()
                .is_some_and(|p| self.pages.get(p).copied().unwrap_or(0) >= b.pages.get())
        {
            return Err(BudgetError::new(K::Exhausted));
        }
        let mut earliest = account.not_before;
        if let Some(last) = account.starts.last() {
            earliest = earliest.max(last.saturating_add(i64::from(account.spacing_ms)));
        }
        let count = account
            .starts
            .iter()
            .filter(|at| **at > now.saturating_sub(60000))
            .count();
        if count >= usize::from(account.starts_per_minute) {
            let oldest = account
                .starts
                .iter()
                .filter(|at| **at > now.saturating_sub(60000))
                .nth(count - usize::from(account.starts_per_minute))
                .ok_or_else(|| BudgetError::new(K::Invalid))?;
            earliest = earliest.max(oldest.saturating_add(60000));
        }
        if earliest >= r.valid_until_ms()? {
            return Err(BudgetError::new(K::Exhausted));
        }
        if now < earliest {
            return Err(BudgetError {
                kind: K::RateLimited,
                source: None,
                retry_at_ms: Some(earliest),
            });
        }
        account
            .daily
            .as_mut()
            .ok_or_else(|| BudgetError::new(K::Denied))?
            .charge(r.request().scope.endpoint, now)?;
        let permit = CallPermit {
            command: r.command().clone(),
            ordinal: self.calls + 1,
            reserved_bytes: b.response_bytes.get(),
        };
        self.calls += 1;
        self.bytes += permit.reserved_bytes;
        if let Some(page) = page {
            *self.pages.entry(page).or_default() += 1;
        }
        self.pending = Some(permit.clone());
        account.pending = Some(permit.clone());
        account.starts.retain(|at| *at > now.saturating_sub(60000));
        account.starts.push(now);
        Ok(permit)
    }
    /// Complete only the exact outstanding fence after confirmed local quiescence.
    /// Can reconcile after cancellation/expiry; it never authorizes another call by itself.
    /// # Errors
    /// Rejects stale/different fences and impossible decoded byte counts without mutation.
    pub fn complete(
        &mut self,
        account: &mut AccountBudget,
        r: &AdmissionRecord,
        permit: &CallPermit,
        outcome: CallCompletion,
        now: i64,
    ) -> Result<()> {
        self.verify(r)?;
        account.validate()?;
        if let Some(closed) = self.completed.iter().find(|c| c.permit == *permit) {
            return if closed.outcome == outcome {
                Ok(())
            } else {
                Err(BudgetError::new(K::Conflict))
            };
        }
        if self.pending.as_ref() != Some(permit)
            || account.pending.as_ref() != Some(permit)
            || &permit.command != r.command()
        {
            return Err(BudgetError::new(K::Conflict));
        }
        if let CallCompletion::Observed { decoded_bytes, .. } = outcome
            && decoded_bytes > permit.reserved_bytes
        {
            return Err(BudgetError::new(K::Invalid));
        }
        // Validate first, then carry a possibly delayed credit debit before releasing
        // the matching fence. An old idempotent completion returned above cannot charge twice.
        let deadline = r.valid_until_ms()?;
        if let Some(daily) = &mut account.daily {
            daily.complete(r.request().scope.endpoint, now)?;
        }
        if let CallCompletion::Observed {
            decoded_bytes,
            retry_after_ms,
        } = outcome
        {
            self.bytes -= permit.reserved_bytes - decoded_bytes;
            if let Some(delay) = retry_after_ms {
                let until = now.saturating_add(i64::from(delay));
                account.not_before = account.not_before.max(until);
                if until >= deadline {
                    self.retry_exhausted = true;
                }
            }
        }
        self.completed.push(ClosedCall {
            permit: permit.clone(),
            outcome,
        });
        self.pending = None;
        account.pending = None;
        Ok(())
    }
    /// Persist cancellation without refunding or releasing unresolved external work.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}
impl CallPermit {
    /// Immutable command reference for owner-private recovery.
    #[must_use]
    pub const fn command(&self) -> &AcquireEvidence {
        &self.command
    }
}
