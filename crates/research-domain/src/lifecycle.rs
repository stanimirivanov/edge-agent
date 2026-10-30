//! Explicit-time append-only lifecycle policy; no clock, transport or storage.
/// Observed state; no transition restores publication eligibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// Original published artifact.
    Published,
    /// Owner invalidation.
    Invalidated,
    /// Explicit expiry cutoff reached.
    Expired,
    /// Final owner withdrawal.
    Withdrawn,
    /// Final link to a separately committed replacement.
    Corrected,
}
/// Consecutive lifecycle head; revision zero is the original artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Head {
    /// Last observed state.
    pub state: State,
    /// Owner lifecycle revision, distinct from projection revision.
    pub revision: u64,
    /// Last effective time; initially the original decision time.
    pub effective_at_ms: i64,
}
/// Payload-free transition refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidTransition;
impl std::fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid artifact lifecycle transition")
    }
}
impl std::error::Error for InvalidTransition {}
impl Head {
    /// Check ordering, explicit expiry and irreversible state. At most two changes exist.
    /// # Errors
    /// Rejects skips, time reversal, premature expiry, repeated or final-state changes.
    pub fn transition(
        self,
        revision: u64,
        effective_at_ms: i64,
        expires_at_ms: i64,
        state: State,
    ) -> Result<Self, InvalidTransition> {
        let legal = match self.state {
            State::Published => state != State::Published,
            State::Invalidated | State::Expired => {
                matches!(state, State::Withdrawn | State::Corrected)
            }
            State::Withdrawn | State::Corrected => false,
        };
        if !legal
            || self.revision.checked_add(1) != Some(revision)
            || revision > 2
            || effective_at_ms < self.effective_at_ms
            || (state == State::Expired && effective_at_ms < expires_at_ms)
        {
            return Err(InvalidTransition);
        }
        Ok(Self {
            state,
            revision,
            effective_at_ms,
        })
    }
}
