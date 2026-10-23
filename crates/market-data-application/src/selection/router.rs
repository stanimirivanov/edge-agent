use super::*;

/// Protocol header classification; adapters validate framing before constructing it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameKind {
    /// Data key is looked up only after scope/sequence checks.
    Data(RoutingKey),
    /// Explicit control evidence; never hidden by an empty selection.
    Heartbeat,
    /// Feed reset invalidates the cursor and directory.
    Reset,
    /// Directory replacement invalidates the old complete snapshot.
    DirectoryChanged,
    /// Authentication/entitlement failure invalidates intake.
    AuthenticationFailure,
}
/// Validated protocol metadata, not a full market-data payload or readiness grant.
#[derive(Clone, Eq, PartialEq)]
pub struct FrameMetadata {
    /// Exact source/feed incarnation from the validated header.
    pub scope: RoutingScope,
    /// Sequence interpreted only under the explicit qualified continuity mode.
    pub sequence: Option<u64>,
    /// Data/control classification must be determined before filtering.
    pub kind: FrameKind,
}
/// Header routing result; discarded data still consumed its control/sequence metadata.
#[must_use]
#[derive(Debug)]
pub enum RouteDecision<'a> {
    /// Decode data using the exact checked canonical mapping.
    Decode(InstrumentRoute<'a>),
    /// Known unselected data: do not decode the payload.
    Skip,
    /// Caller must propagate heartbeat evidence to the existing P04 tracker.
    Heartbeat,
}
/// Single-owner header cursor. This is not P04 recovery, source qualification or readiness.
/// Any error latches invalidity. Only externally qualified reconstruction can resume intake.
pub struct EarlyRouter<'a> {
    plan: &'a SelectionPlan<'a>,
    mode: ContinuityMode,
    valid: bool,
}
impl<'a> EarlyRouter<'a> {
    /// Use an explicit qualified feed baseline, or snapshots without a gap-free claim.
    /// # Errors
    /// Rejects unsupported future modes rather than inferring continuity.
    pub fn new(plan: &'a SelectionPlan<'a>, mode: ContinuityMode) -> Result<Self> {
        if !matches!(
            mode,
            ContinuityMode::Snapshots | ContinuityMode::Contiguous { .. }
        ) {
            return Err(SelectionError::Scope);
        }
        Ok(Self {
            plan,
            mode,
            valid: true,
        })
    }
    /// Routing remains usable; this does not establish heartbeat freshness or data readiness.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.valid
    }
    /// Fail closed after adapter framing/payload errors. There is no in-place heal operation.
    pub fn invalidate(&mut self) {
        self.valid = false;
    }
    /// Process header metadata/control before consulting selection; full decoding is external.
    /// # Errors
    /// Invalidates on unknown mappings, foreign epochs, continuity or control failures.
    pub fn route(&mut self, header: &FrameMetadata) -> Result<RouteDecision<'a>> {
        let result = self.checked(header);
        if result.is_err() {
            self.invalidate();
        }
        result
    }
    fn checked(&mut self, header: &FrameMetadata) -> Result<RouteDecision<'a>> {
        if !self.valid {
            return Err(SelectionError::Invalidated);
        }
        if header.scope != self.plan.context.scope {
            return Err(SelectionError::Scope);
        }
        if let ContinuityMode::Contiguous { next } = self.mode {
            if header.sequence != Some(next) {
                return Err(SelectionError::Continuity);
            }
            self.mode = ContinuityMode::Contiguous {
                next: next.checked_add(1).ok_or(SelectionError::Continuity)?,
            };
        }
        let key = match &header.kind {
            FrameKind::Heartbeat => return Ok(RouteDecision::Heartbeat),
            FrameKind::Reset => return Err(SelectionError::Reset),
            FrameKind::DirectoryChanged => return Err(SelectionError::DirectoryChanged),
            FrameKind::AuthenticationFailure => return Err(SelectionError::Authentication),
            FrameKind::Data(key) => key,
        };
        let mapping = self
            .plan
            .directory
            .value()
            .entries
            .get(key)
            .ok_or(SelectionError::Mapping)?;
        if self
            .plan
            .selection
            .value()
            .members
            .contains(&mapping.value().identities().0)
        {
            Ok(RouteDecision::Decode(InstrumentRoute(mapping)))
        } else {
            Ok(RouteDecision::Skip)
        }
    }
}
redact!(FrameMetadata, EarlyRouter<'_>);
