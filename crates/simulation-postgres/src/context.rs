use super::{invariant, permanent};
use edgeagent_messaging::HandlerFailure;
use edgeagent_simulation_domain::{
    ArtifactDigest, RunManifest,
    admission::SyntheticReservationQuote,
    risk::{RiskAccountId, RiskPolicy, RiskSession, RiskState},
};
use std::{collections::BTreeSet, time::Duration};

/// Explicit transaction-local budgets, without truncation or permissive defaults.
#[derive(Clone, Copy, Debug)]
pub struct AdmissionLimits {
    pub(crate) lock_ms: u64,
    pub(crate) statement_ms: u64,
}
impl AdmissionLimits {
    /// Require whole milliseconds in 1..=30000 and statement >= lock timeout.
    ///
    /// # Errors
    /// Reject invalid/overflowing/sub-millisecond budgets as startup configuration.
    pub fn new(lock: Duration, statement: Duration) -> Result<Self, HandlerFailure> {
        let whole = |value: Duration| -> Result<u64, HandlerFailure> {
            let millis = u64::try_from(value.as_millis()).map_err(|_| invariant())?;
            if !(1..=30_000).contains(&millis) || value != Duration::from_millis(millis) {
                return Err(invariant());
            }
            Ok(millis)
        };
        let lock_ms = whole(lock)?;
        let statement_ms = whole(statement)?;
        if statement_ms < lock_ms {
            return Err(invariant());
        }
        Ok(Self {
            lock_ms,
            statement_ms,
        })
    }
}
/// Immutable owner context. The principal MUST come from verified composition,
/// not JSON, an LLM or a CloudEvents source string. DB grants are checked under
/// the account lock. Frozen catalogs have no provider-qualified quote constructor.
pub struct AdmissionContext<'run> {
    pub(crate) account: RiskAccountId,
    pub(crate) principal: String,
    pub(crate) manifest: &'run RunManifest,
    pub(crate) policy: RiskPolicy,
    pub(crate) session: RiskSession,
    pub(crate) quotes: Vec<SyntheticReservationQuote>,
    pub(crate) limits: AdmissionLimits,
    pub(crate) configuration: ArtifactDigest,
    pub(crate) initial_run: Vec<u8>,
    pub(crate) execution_slots: Vec<(u64, edgeagent_market_data_domain::PreciseInstant)>,
}
impl<'run> AdmissionContext<'run> {
    /// Bind 1..=4096 distinct immutable synthetic quotes and canonical input slots.
    /// Caller retains/verifies original manifest, policy and synthetic artifacts.
    ///
    /// # Errors
    /// Reject invalid identity, foreign account/run, duplicates or invalid bounds.
    pub fn new(
        account: RiskAccountId,
        principal: u128,
        manifest: &'run RunManifest,
        policy: RiskPolicy,
        session: RiskSession,
        quotes: &[SyntheticReservationQuote],
        limits: AdmissionLimits,
    ) -> Result<Self, HandlerFailure> {
        Self::new_scheduled(
            account,
            principal,
            manifest,
            policy,
            session,
            (quotes, &[]),
            limits,
        )
    }
    /// Bind a combined admission/execution schedule. Every canonical slot must
    /// occur exactly once, starting at 1, with nonregressing virtual time. The
    /// execution context subsequently binds the actual actions and evidence.
    ///
    /// # Errors
    /// Reject gaps, overlapping slots and invalid quote/context configuration.
    pub fn new_scheduled(
        account: RiskAccountId,
        principal: u128,
        manifest: &'run RunManifest,
        policy: RiskPolicy,
        session: RiskSession,
        catalog: (
            &[SyntheticReservationQuote],
            &[(u64, edgeagent_market_data_domain::PreciseInstant)],
        ),
        limits: AdmissionLimits,
    ) -> Result<Self, HandlerFailure> {
        let (quotes, execution_slots) = catalog;
        if principal == 0
            || quotes.is_empty()
            || quotes.len().saturating_add(execution_slots.len()) > 4096
        {
            return Err(permanent("admission_configuration"));
        }
        let mut identities = BTreeSet::new();
        let mut sequences = BTreeSet::new();
        let mut orders = BTreeSet::new();
        for quote in quotes {
            let scope = quote.order().scope().identity();
            if scope.0 != manifest.digest()
                || scope.1 != account
                || quote.canonical_bytes().len() > 4096
                || !identities.insert(quote.fingerprint().bytes())
                || !sequences.insert(quote.position().0)
                || !orders.insert(scope.3.bytes())
            {
                return Err(invariant());
            }
            super::signed(quote.cash().map_err(|_| invariant())?.get())?;
            super::signed(quote.order().terms().2.parts().1)?;
        }
        let mut quotes = quotes.to_vec();
        quotes.sort_by_key(|quote| quote.position().0);
        let mut schedule = quotes
            .iter()
            .map(SyntheticReservationQuote::position)
            .collect::<Vec<_>>();
        schedule.extend_from_slice(execution_slots);
        schedule.sort_by_key(|slot| slot.0);
        if schedule.len() > 4096
            || schedule
                .iter()
                .enumerate()
                .any(|(i, slot)| slot.0 != i as u64 + 1)
            || schedule
                .windows(2)
                .any(|pair| matches!(pair, [a,b] if a.1>b.1))
        {
            return Err(invariant());
        }
        let mut initial_run = edgeagent_simulation_domain::Run::new(manifest);
        for (index, quote) in quotes.iter().enumerate() {
            if quote.order().scope().identity().3.bytes()
                != initial_run.next_id().map_err(|_| invariant())?.bytes()
                || index
                    .checked_sub(1)
                    .and_then(|previous| quotes.get(previous))
                    .is_some_and(|previous| previous.position().1 > quote.position().1)
            {
                return Err(invariant());
            }
        }
        let initial_run = initial_run.checkpoint();
        let start = manifest
            .inputs()
            .first()
            .ok_or_else(invariant)?
            .available_at();
        let risk =
            RiskState::new(account, policy.clone(), session, start).map_err(|_| invariant())?;
        let mut bytes = manifest.canonical_bytes().to_vec();
        bytes.extend(risk.checkpoint());
        bytes.extend(limits.lock_ms.to_be_bytes());
        bytes.extend(limits.statement_ms.to_be_bytes());
        if !execution_slots.is_empty() {
            bytes.extend(b"EA-COMBINED-SLOTS1\0");
            bytes.extend((quotes.len() as u64).to_be_bytes());
            bytes.extend((execution_slots.len() as u64).to_be_bytes());
        }
        for quote in &quotes {
            bytes.extend(quote.fingerprint().bytes());
        }
        for (slot, at) in execution_slots {
            bytes.extend(slot.to_be_bytes());
            let (seconds, nanos) = at.unix_parts();
            bytes.extend(seconds.to_be_bytes());
            bytes.extend(nanos.to_be_bytes());
        }
        Ok(Self {
            account,
            principal: format!("{principal:032x}"),
            manifest,
            policy,
            session,
            quotes,
            limits,
            configuration: ArtifactDigest::of(&bytes),
            initial_run,
            execution_slots: execution_slots.to_vec(),
        })
    }
    /// Account identity for private-schema provisioning and command routing.
    #[must_use]
    pub fn account(&self) -> String {
        format!("{:032x}", self.account.get())
    }
    /// Immutable scenario/configuration binding, excluding the authenticated actor.
    #[must_use]
    pub const fn configuration(&self) -> ArtifactDigest {
        self.configuration
    }
}
impl std::fmt::Debug for AdmissionContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdmissionContext")
            .field("account", &self.account)
            .field("quotes", &self.quotes.len())
            .finish_non_exhaustive()
    }
}

// P05 IDs are unique inside a run. Outbox source/message identity spans every
// account in the service, including accounts replaying the same frozen inputs.
pub(super) fn event_identity(
    account: RiskAccountId,
    run: ArtifactDigest,
    event: [u8; 32],
) -> String {
    let mut bytes = b"EA-ACCOUNT-ADMISSION-EVENT1\0".to_vec();
    bytes.extend(account.get().to_be_bytes());
    bytes.extend(run.bytes());
    bytes.extend(event);
    super::hex(&ArtifactDigest::of(&bytes).bytes())
}
