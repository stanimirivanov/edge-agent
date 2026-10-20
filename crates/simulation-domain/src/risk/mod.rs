//! Versioned, conservative dry-run risk policy and revision-checked recovery.
//! These pure decisions are not admission tokens. P08 must enforce them against
//! complete authoritative state under the account transaction, never projections.
//!
//! Account and strategy identity namespaces cannot cross:
//! ```compile_fail,E0308
//! use edgeagent_simulation_domain::risk::{RiskAccountId, RiskStrategyId};
//! fn account(_: RiskAccountId) {}
//! fn invalid(strategy: RiskStrategyId) { account(strategy); }
//! ```
//!
//! Synthetic single-strategy account; a proposal denial is not an actual breach:
//! ```
//! use edgeagent_simulation_domain::risk::*;
//! use edgeagent_market_data_domain::{PreciseInstant, EvidenceInterval,
//!     ReferenceId, ReferenceRevision, SemanticReference, SessionDate};
//! use std::time::Duration;
//! let start = PreciseInstant::from_unix_parts(0, 0)?;
//! let now = PreciseInstant::from_unix_parts(1, 0)?;
//! let rules = SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?);
//! let session = RiskSession::new(SessionDate::new(1970, 1, 1)?,
//!     EvidenceInterval::new(start, PreciseInstant::from_unix_parts(100, 0)?)?, rules);
//! let strategy = RiskStrategyId::new(1)?;
//! let account = RiskAccountId::new(1)?;
//! let limits = RiskLimits::new(UsdCents::new(15), UsdCents::new(5), 2);
//! let policy = RiskPolicy::new(rules, Duration::from_secs(1), limits,
//!     &[(strategy, limits)], &[RiskOperatorId::new(7)?])?;
//! let mut state = RiskState::new(account, policy, session, start)?;
//! let usage = RiskUsage::new(UsdCents::new(10), UsdCents::new(5), UsdCents::new(0), 1)?;
//! let snapshot = RiskSnapshot::new(account, session,
//!     &[StrategyValuation::new(strategy, usage, now)])?;
//! assert_eq!(state.observe(1, &snapshot, now)?, RiskDecision::Permitted);
//! let proposal = RiskIncrease::new(strategy, UsdCents::new(1), 1);
//! assert_eq!(state.check_increase(2, &snapshot, now, proposal)?,
//!     RiskDecision::Denied(RiskReason::AccountGross));
//! assert_eq!(state.status(), (3, None));
//! // P08 must reserve/commit under the account lock; this is not admission.
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod policy;
mod state;
mod values;

pub use policy::{RiskPolicy, RiskSnapshot, StrategyValuation};
pub use state::{RiskRecovery, RiskRecoveryKind, RiskState};
pub use values::{
    RiskAccountId, RiskDecision, RiskError, RiskIncrease, RiskLimits, RiskOperatorId, RiskReason,
    RiskSession, RiskStrategyId, RiskUsage, UsdCents,
};
