use super::{
    RiskAccountId, RiskDecision, RiskError, RiskIncrease, RiskOperatorId, RiskPolicy, RiskReason,
    RiskSession, RiskSnapshot,
};
use edgeagent_market_data_domain::{PreciseInstant, SemanticReference};

mod checkpoint;

#[cfg(test)]
mod tests;

/// Auditable recovery operation, distinct from normal risk observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RiskRecoveryKind {
    /// Clear a current-session latch after a healthy authorized assessment.
    Reset,
    /// Move forward to an explicit session WITHOUT clearing the breach latch.
    Rollover,
}
/// Recovery audit facts to retain with the state transition in P08's transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct RiskRecovery {
    actor: RiskOperatorId,
    revision: u64,
    policy: SemanticReference,
    fingerprint: crate::ArtifactDigest,
    kind: RiskRecoveryKind,
    at: PreciseInstant,
    session: RiskSession,
}
impl RiskRecovery {
    /// Exact content of the pinned policy, independent of its assigned revision.
    #[must_use]
    pub const fn policy_fingerprint(self) -> crate::ArtifactDigest {
        self.fingerprint
    }
    /// Actor, resulting risk revision, pinned policy and operation kind.
    #[must_use]
    pub const fn audit(self) -> (RiskOperatorId, u64, SemanticReference, RiskRecoveryKind) {
        (self.actor, self.revision, self.policy, self.kind)
    }
    /// Exact decision time and resulting calendar-bound session.
    #[must_use]
    pub const fn context(self) -> (PreciseInstant, RiskSession) {
        (self.at, self.session)
    }
}

/// Single-owner, policy-pinned breach state. No public latch mutation or automatic
/// reset. Application persistence must fence the account and retain revisions/audit.
/// This value does not authenticate actors or guarantee a snapshot's completeness.
#[derive(Debug)]
pub struct RiskState {
    account: RiskAccountId,
    policy: RiskPolicy,
    session: RiskSession,
    revision: u64,
    clock: PreciseInstant,
    breach: Option<RiskReason>,
}
impl RiskState {
    /// Start risk revision 1, unlatched, inside an explicit qualified session.
    ///
    /// # Errors
    /// Returns `Session` when start time is outside the half-open session.
    pub fn new(
        account: RiskAccountId,
        policy: RiskPolicy,
        session: RiskSession,
        at: PreciseInstant,
    ) -> Result<Self, RiskError> {
        if !session.contains(at) {
            return Err(RiskError::Session);
        }
        Ok(Self {
            account,
            policy,
            session,
            revision: 1,
            clock: at,
            breach: None,
        })
    }
    /// Risk revision and original latched reason (if any). Persist both atomically.
    #[must_use]
    pub const fn status(&self) -> (u64, Option<RiskReason>) {
        (self.revision, self.breach)
    }
    /// Observe actual account usage. Current breaches latch; improved data cannot
    /// clear a latch. Successful assessments (including denials) advance revision.
    ///
    /// # Errors
    /// Rejects stale revision, wrong scope, regressing/future time or overflow;
    /// every error leaves state unchanged and grants no admission.
    pub fn observe(
        &mut self,
        expected: u64,
        snapshot: &RiskSnapshot,
        at: PreciseInstant,
    ) -> Result<RiskDecision, RiskError> {
        self.apply(expected, snapshot, at, None)
    }
    /// Check a proposed increase after current-account health; prospective cap
    /// denial alone does not latch an otherwise healthy account.
    ///
    /// # Errors
    /// Same as `observe`; unknown target strategies also reject without mutation.
    pub fn check_increase(
        &mut self,
        expected: u64,
        snapshot: &RiskSnapshot,
        at: PreciseInstant,
        increase: RiskIncrease,
    ) -> Result<RiskDecision, RiskError> {
        self.apply(expected, snapshot, at, Some(increase))
    }
    fn apply(
        &mut self,
        expected: u64,
        snapshot: &RiskSnapshot,
        at: PreciseInstant,
        increase: Option<RiskIncrease>,
    ) -> Result<RiskDecision, RiskError> {
        self.validate(expected, at)?;
        self.scope(snapshot)?;
        let current = self.policy.current(snapshot, at)?;
        let projected = if let Some(increase) = increase {
            self.policy.projected(snapshot, increase)?
        } else {
            None
        };
        let revision = self.next_revision()?;
        let breach = self.breach.or(current);
        let decision = breach
            .or(projected)
            .map_or(RiskDecision::Permitted, RiskDecision::Denied);
        self.breach = breach;
        self.revision = revision;
        self.clock = at;
        Ok(decision)
    }
    /// Explicitly clear a latch only for an authorized actor with fresh healthy
    /// current-session evidence. Authentication/actor mapping belongs to the adapter.
    ///
    /// # Errors
    /// Rejects unauthorized/stale commands, unhealthy evidence, absent latch,
    /// wrong scope/time or overflow. No rejected reset changes state.
    pub fn reset(
        &mut self,
        expected: u64,
        actor: RiskOperatorId,
        snapshot: &RiskSnapshot,
        at: PreciseInstant,
    ) -> Result<RiskRecovery, RiskError> {
        self.authorize(expected, actor, at)?;
        self.scope(snapshot)?;
        if self.breach.is_none() || self.policy.current(snapshot, at)?.is_some() {
            return Err(RiskError::Reset);
        }
        let receipt = self.recovery(actor, RiskRecoveryKind::Reset, at, self.session)?;
        self.breach = None;
        Ok(receipt)
    }
    /// Authorize a forward non-overlapping calendar-bound session. This does NOT
    /// clear a latch, mutate holdings/reservations, or establish a loss baseline.
    /// P08 must carry all exposure and establish the new baseline atomically.
    ///
    /// # Errors
    /// Rejects unauthorized/stale commands, regressing time, non-forward/overlapping
    /// sessions, inactive target sessions or revision overflow, without mutation.
    pub fn rollover(
        &mut self,
        expected: u64,
        actor: RiskOperatorId,
        next: RiskSession,
        at: PreciseInstant,
    ) -> Result<RiskRecovery, RiskError> {
        self.authorize(expected, actor, at)?;
        let (_, end) = self.session.interval.bounds();
        let (start, _) = next.interval.bounds();
        if next.date <= self.session.date || start < end || at < end || !next.contains(at) {
            return Err(RiskError::Session);
        }
        self.recovery(actor, RiskRecoveryKind::Rollover, at, next)
    }
    fn validate(&self, expected: u64, at: PreciseInstant) -> Result<(), RiskError> {
        if expected != self.revision {
            return Err(RiskError::Revision);
        }
        if at < self.clock {
            return Err(RiskError::Clock);
        }
        Ok(())
    }
    fn scope(&self, snapshot: &RiskSnapshot) -> Result<(), RiskError> {
        if snapshot.account != self.account || snapshot.session != self.session {
            return Err(RiskError::Scope);
        }
        Ok(())
    }
    fn authorize(
        &self,
        expected: u64,
        actor: RiskOperatorId,
        at: PreciseInstant,
    ) -> Result<(), RiskError> {
        if !self.policy.operators.contains(&actor) {
            return Err(RiskError::Unauthorized);
        }
        self.validate(expected, at)
    }
    fn next_revision(&self) -> Result<u64, RiskError> {
        self.revision.checked_add(1).ok_or(RiskError::Overflow)
    }
    fn recovery(
        &mut self,
        actor: RiskOperatorId,
        kind: RiskRecoveryKind,
        at: PreciseInstant,
        session: RiskSession,
    ) -> Result<RiskRecovery, RiskError> {
        let revision = self.next_revision()?;
        let receipt = RiskRecovery {
            actor,
            revision,
            policy: self.policy.revision,
            fingerprint: self.policy.fingerprint(),
            kind,
            at,
            session,
        };
        self.revision = revision;
        self.clock = at;
        self.session = session;
        Ok(receipt)
    }
}
