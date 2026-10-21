use super::*;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Exact descriptive metrics for ONE synthetic path, not statistical evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CaseMetrics {
    /// Predeclared factor. Case order, not returns, determines report order.
    pub axis: SensitivityAxis,
    /// SHA-256 of complete explicit execution profile bytes.
    pub profile: String,
    /// SHA-256 over all canonical owner frames and run continuations, in order.
    pub trace: String,
    /// Number of transitions, excluding genesis; not independent trades/samples.
    pub transitions: u64,
    /// Number of order identities, including rejected/unfilled orders.
    pub orders: u64,
    /// Filled shares and contracts separately; NEVER add incompatible units.
    pub filled_units: [u64; 2],
    /// Cumulative explicit execution AND settlement fee expense, in USD cents.
    pub fees_cents: u64,
    /// Realized P/L less fees, in USD cents. Requires closed terminal inventory.
    pub net_cents: i128,
    /// Sum of absolute cash flows excluding fees, including settlement proceeds.
    /// This is not trade turnover (no denominator, no fabricated market marks).
    pub gross_cash_flow_cents: u128,
    /// Peak-to-trough cash-plus-book-cost decline in USD cents. NOT MTM drawdown.
    pub book_drawdown_cents: u128,
    /// Whether any retained account revision reports a sticky risk breach.
    pub risk_breached: bool,
    /// Paired net result minus baseline in cents; zero for baseline itself.
    pub net_delta_cents: i128,
    /// Paired fee expense minus baseline in cents.
    pub fee_delta_cents: i128,
}

/// Immutable canonical output. Retain bytes and their digest with the frozen plan;
/// do not overwrite earlier experiments or interpret hashes as authenticity.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct EvaluationReport {
    version: &'static str,
    plan: String,
    paths_per_case: u64,
    uncertainty: &'static str,
    cases: Vec<CaseMetrics>,
}
impl std::fmt::Debug for EvaluationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvaluationReport")
            .field("cases", &self.cases.len())
            .finish_non_exhaustive()
    }
}
impl EvaluationReport {
    pub(super) fn new(
        plan: ArtifactDigest,
        mut cases: Vec<CaseMetrics>,
    ) -> Result<Self, EvaluationError> {
        let first = cases.first().ok_or(EvaluationError::Incomplete)?;
        let (net, fees) = (first.net_cents, first.fees_cents);
        for case in &mut cases {
            case.net_delta_cents = case.net_cents - net;
            case.fee_delta_cents = i128::from(case.fees_cents) - i128::from(fees);
        }
        Ok(Self {
            version: "edgeagent.frozen-evaluation.v1",
            plan: hex(plan.bytes()),
            paths_per_case: 1,
            uncertainty: "not_estimated_single_synthetic_path",
            cases,
        })
    }
    /// Results in predeclared case order, with no winner selection or sorting.
    #[must_use]
    pub fn cases(&self) -> &[CaseMetrics] {
        &self.cases
    }
    /// Canonical JSON, bounded to 64 KiB. No decoder bypasses evaluation checks.
    ///
    /// # Errors
    /// Returns `Capacity` on encoding failure or excessive output size.
    pub fn encode(&self) -> Result<Vec<u8>, EvaluationError> {
        let bytes = serde_json::to_vec(self).map_err(|_| EvaluationError::Capacity)?;
        if bytes.len() > 65_536 {
            return Err(EvaluationError::Capacity);
        }
        Ok(bytes)
    }
}

pub(super) fn evaluate(
    plan: &EvaluationPlan<'_>,
    case: &EvaluationCase,
    trace: &EvaluationTrace<'_>,
) -> Result<CaseMetrics, EvaluationError> {
    if trace.owner_revision != case.revisions || trace.samples.len() as u64 != case.revisions + 1 {
        return Err(EvaluationError::Incomplete);
    }
    let genesis = trace
        .samples
        .first()
        .ok_or(EvaluationError::Incomplete)?
        .frame;
    let mut projection = AccountProjection::new(genesis.clone(), trace.samples.len())
        .map_err(|_| EvaluationError::Fidelity)?;
    let initial = equity(genesis.state())?;
    let mut peak = initial;
    let mut drawdown = 0;
    let mut flow = 0_u128;
    let mut breached = false;
    let mut previous_cash = i128::from(genesis.state().cash_cents);
    let mut previous_fee = 0_i128;
    let mut previous_time = Run::new(plan.manifest).now();
    let mut previous_cursor = 0;
    let mut hash = Sha256::new();
    hash.update(b"EA-EVALUATION-TRACE1\0");
    let mut size = 0_usize;
    for (index, sample) in trace.samples.iter().enumerate() {
        let state = sample.frame.state();
        if state.revision != index as u64 {
            return Err(EvaluationError::Incomplete);
        }
        if state.run != hex(plan.manifest.digest().bytes())
            || state.configuration != hex(case.configuration.bytes())
            || state.execution_plan.as_deref() != Some(hex(case.execution.bytes()).as_str())
            || state.settlement_plan != case.settlement.map(|d| hex(d.bytes()))
        {
            return Err(EvaluationError::Scope);
        }
        let run = Run::restore(plan.manifest, sample.checkpoint)
            .map_err(|_| EvaluationError::Fidelity)?;
        if run.now() < previous_time
            || run.now() > plan.end
            || run.consumed_inputs().len() < previous_cursor
            || state
                .valuations
                .iter()
                .any(|v| (v.valued_seconds, u64::from(v.valued_nanos)) > run.now().unix_parts())
        {
            return Err(EvaluationError::Chronology);
        }
        // Owner bootstrap preallocates deterministic order IDs. Genesis must
        // precede market inputs, but is NOT necessarily a zero ID/RNG counter.
        if index == 0
            && (run.now() != Run::new(plan.manifest).now() || !run.consumed_inputs().is_empty())
        {
            return Err(EvaluationError::Fidelity);
        }
        if index as u64 == case.revisions && (run.peek().is_some() || run.now() != plan.end) {
            return Err(EvaluationError::Incomplete);
        }
        previous_time = run.now();
        previous_cursor = run.consumed_inputs().len();
        let bytes = sample
            .frame
            .encode()
            .map_err(|_| EvaluationError::Fidelity)?;
        size = size
            .checked_add(bytes.len())
            .and_then(|n| n.checked_add(sample.checkpoint.len()))
            .ok_or(EvaluationError::Capacity)?;
        if size > 16 * 1024 * 1024 {
            return Err(EvaluationError::Capacity);
        }
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
        hash.update((sample.checkpoint.len() as u64).to_be_bytes());
        hash.update(sample.checkpoint);
        if index != 0 {
            projection
                .apply(sample.frame.clone())
                .map_err(|_| EvaluationError::Fidelity)?;
        }
        let value = equity(state)?;
        peak = peak.max(value);
        drawdown = drawdown.max(peak - value);
        let cash = i128::from(state.cash_cents);
        let fee = i128::from(state.postings[3]);
        flow = flow
            .checked_add((cash - previous_cash + fee - previous_fee).unsigned_abs())
            .ok_or(EvaluationError::Capacity)?;
        previous_cash = cash;
        previous_fee = fee;
        breached |= state.risk_breach.is_some();
    }
    let last = projection.state();
    if last
        .positions
        .iter()
        .any(|p| p.quantity != 0 || p.cost_cents != Some(0))
        || last.orders.iter().any(|o| {
            !matches!(
                o.status.as_str(),
                "filled" | "cancelled" | "expired" | "rejected"
            ) || o.reserved_cash_cents != 0
                || o.reserved_units != 0
        })
    {
        return Err(EvaluationError::Incomplete);
    }
    let mut units = [0_u64; 2];
    for order in &last.orders {
        let index = match order.unit.as_str() {
            "shares" => 0,
            "contracts" => 1,
            _ => return Err(EvaluationError::Fidelity),
        };
        units[index] = units[index]
            .checked_add(order.filled)
            .ok_or(EvaluationError::Capacity)?;
    }
    let fees = u64::try_from(last.postings[3]).map_err(|_| EvaluationError::Fidelity)?;
    let net = -i128::from(last.postings[2]) - i128::from(fees);
    if i128::from(last.cash_cents)
        - i128::try_from(initial).map_err(|_| EvaluationError::Capacity)?
        != net
    {
        return Err(EvaluationError::Fidelity);
    }
    Ok(CaseMetrics {
        axis: case.axis,
        profile: hex(case.profile.identity().1.bytes()),
        trace: hex(hash.finalize().into()),
        transitions: case.revisions,
        orders: last.orders.len() as u64,
        filled_units: units,
        fees_cents: fees,
        net_cents: net,
        gross_cash_flow_cents: flow,
        book_drawdown_cents: drawdown,
        risk_breached: breached,
        net_delta_cents: 0,
        fee_delta_cents: 0,
    })
}

fn equity(state: &edgeagent_contracts::audit::AccountAuditState) -> Result<u128, EvaluationError> {
    state
        .positions
        .iter()
        .try_fold(u128::from(state.cash_cents), |sum, p| {
            sum.checked_add(u128::from(p.cost_cents.ok_or(EvaluationError::Incomplete)?))
                .ok_or(EvaluationError::Capacity)
        })
}
fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
