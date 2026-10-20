use super::{RiskAccountId, RiskError, RiskPolicy, RiskReason, RiskSession, RiskState};
use crate::{ArtifactDigest, risk::RiskStrategyId};
use edgeagent_market_data_domain::PreciseInstant;

impl RiskState {
    /// Versioned continuation retaining revision, time and the original breach.
    /// Commit with all account effects. Its checksum detects corruption, not an
    /// authorized rollback, authenticity or distributed ownership.
    #[must_use]
    pub fn checkpoint(&self) -> Vec<u8> {
        let mut out = binding(self.account, &self.policy, self.session);
        out.extend(self.revision.to_be_bytes());
        let (seconds, nanos) = self.clock.unix_parts();
        out.extend(seconds.to_be_bytes());
        out.extend(nanos.to_be_bytes());
        let (code, strategy) = reason(self.breach);
        out.push(code);
        out.extend(strategy.to_be_bytes());
        out.extend(ArtifactDigest::of(&out).bytes());
        out
    }
    /// Restore only a complete authoritative checkpoint under the identical policy
    /// content, account and calendar-bound session. Never reinitialize on failure.
    ///
    /// # Errors
    /// Returns `Configuration` for malformed, mismatched, corrupted or impossible
    /// state. The persistence adapter must separately fence and authenticate it.
    pub fn restore(
        account: RiskAccountId,
        policy: RiskPolicy,
        session: RiskSession,
        bytes: &[u8],
    ) -> Result<Self, RiskError> {
        let prefix = binding(account, &policy, session);
        let (body, checksum) = bytes
            .split_at_checked(126)
            .ok_or(RiskError::Configuration)?;
        if checksum.len() != 32
            || ArtifactDigest::of(body).bytes().as_slice() != checksum
            || body.get(..prefix.len()) != Some(prefix.as_slice())
        {
            return Err(RiskError::Configuration);
        }
        let mut payload = body.get(prefix.len()..).ok_or(RiskError::Configuration)?;
        let revision = u64::from_be_bytes(take(&mut payload)?);
        let seconds = i64::from_be_bytes(take(&mut payload)?);
        let nanos = u64::from_be_bytes(take(&mut payload)?);
        let clock = PreciseInstant::from_unix_parts(
            seconds,
            u32::try_from(nanos).map_err(|_| RiskError::Configuration)?,
        )
        .map_err(|_| RiskError::Configuration)?;
        let code = take::<1>(&mut payload)?[0];
        let strategy = u128::from_be_bytes(take(&mut payload)?);
        if revision == 0 || clock < session.interval.bounds().0 {
            return Err(RiskError::Configuration);
        }
        let breach = decode_reason(code, strategy, &policy)?;
        Ok(Self {
            account,
            policy,
            session,
            revision,
            clock,
            breach,
        })
    }
}
fn binding(account: RiskAccountId, policy: &RiskPolicy, session: RiskSession) -> Vec<u8> {
    let mut descriptor = Vec::new();
    let (year, month, day) = session.date.ymd();
    descriptor.extend(year.to_be_bytes());
    descriptor.extend(month.to_be_bytes());
    descriptor.extend(day.to_be_bytes());
    for instant in [session.interval.bounds().0, session.interval.bounds().1] {
        let (seconds, nanos) = instant.unix_parts();
        descriptor.extend(seconds.to_be_bytes());
        descriptor.extend(nanos.to_be_bytes());
    }
    let (id, revision) = session.calendar.version();
    descriptor.extend(id.get().to_be_bytes());
    descriptor.extend(revision.get().to_be_bytes());
    let mut out = b"EARS1".to_vec();
    out.extend(account.get().to_be_bytes());
    out.extend(policy.fingerprint().bytes());
    out.extend(ArtifactDigest::of(&descriptor).bytes());
    out
}
fn reason(reason: Option<RiskReason>) -> (u8, u128) {
    match reason {
        None => (0, 0),
        Some(RiskReason::SessionClosed) => (1, 0),
        Some(RiskReason::StaleValuation) => (2, 0),
        Some(RiskReason::AccountGross) => (3, 0),
        Some(RiskReason::AccountLoss) => (4, 0),
        Some(RiskReason::AccountOrders) => (5, 0),
        Some(RiskReason::StrategyGross(id)) => (6, id.get()),
        Some(RiskReason::StrategyLoss(id)) => (7, id.get()),
        Some(RiskReason::StrategyOrders(id)) => (8, id.get()),
    }
}
fn decode_reason(
    code: u8,
    strategy: u128,
    policy: &RiskPolicy,
) -> Result<Option<RiskReason>, RiskError> {
    if code <= 5 && strategy != 0 {
        return Err(RiskError::Configuration);
    }
    Ok(match code {
        0 => None,
        1 => Some(RiskReason::SessionClosed),
        2 => Some(RiskReason::StaleValuation),
        3 => Some(RiskReason::AccountGross),
        4 => Some(RiskReason::AccountLoss),
        5 => Some(RiskReason::AccountOrders),
        6..=8 => {
            let id = RiskStrategyId::new(strategy)?;
            if !policy.includes_strategy(id) {
                return Err(RiskError::Configuration);
            }
            Some(match code {
                6 => RiskReason::StrategyGross(id),
                7 => RiskReason::StrategyLoss(id),
                _ => RiskReason::StrategyOrders(id),
            })
        }
        _ => return Err(RiskError::Configuration),
    })
}
fn take<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], RiskError> {
    let (head, tail) = input.split_at_checked(N).ok_or(RiskError::Configuration)?;
    *input = tail;
    head.try_into().map_err(|_| RiskError::Configuration)
}
