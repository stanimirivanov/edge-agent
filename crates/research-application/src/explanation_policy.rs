//! Sealed, independently reconciled policy preview. Owner authentication stays with callers.
#[cfg(test)]
mod tests;
use crate::{
    execution::Selection,
    explanation::Error,
    explanation_intent::Preview,
    governed::{ConstraintInputs, ContextInputs},
    worker::ExecutionEvidence,
};
use edgeagent_contracts::{
    research::ResearchRecord,
    research_decision::DecisionProvenance,
    strategy::{Configuration, ExecutionConfiguration, Regime},
};
use edgeagent_research_domain::{context_policy as context, liquidity, market_constraints};
use std::fmt::Write as _;
/// New preview identity; never silently replaces legacy renderer or qualification pins.
pub const REVISION: &str = "research-explanation-policy.v1";
/// Model choices cannot populate or override the independently computed measurements.
pub struct PolicyPreview<'a> {
    base: Preview<'a>,
    measurements: String,
}
fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn regime(value: context::Regime) -> &'static str {
    match value {
        context::Regime::Falling => "falling",
        context::Regime::Flat => "flat",
        context::Regime::Rising => "rising",
    }
}
impl<'a> PolicyPreview<'a> {
    /// Authenticate all owner assertions before calling. Reconcile exact retained bytes
    /// and decision, then reuse the compiled M05 formulas over the same selected window.
    /// # Errors
    /// Refuses mismatched configuration, incomplete/unknown evidence, or any reconciliation failure.
    pub fn from_verified(
        record: &'a ResearchRecord,
        decision: &'a DecisionProvenance,
        configuration: &[u8],
        input: &ExecutionEvidence,
    ) -> Result<Self, Error> {
        let base = Preview::from_verified(record, decision)?;
        let selection = Selection::decode(configuration).map_err(|_| Error)?;
        selection.verify_provenance(decision).map_err(|_| Error)?;
        let rebuilt = selection
            .validate(
                decision.request.clone(),
                &input.loaded.policy,
                &input.loaded.series,
                input.loaded.expires_at_ms,
                ContextInputs {
                    constraints: ConstraintInputs {
                        volumes: &input.volumes,
                        opens: &input.opens,
                    },
                    context: &input.context,
                },
            )
            .map_err(|_| Error)?;
        if rebuilt.record().canonical_bytes().map_err(|_| Error)?
            != record.canonical_bytes().map_err(|_| Error)?
            || rebuilt.provenance().map_err(|_| Error)? != *decision
        {
            return Err(Error);
        }
        let ExecutionConfiguration::V1 {
            bundle,
            context_max_age_ms,
            allowed_regimes,
            ..
        } = ExecutionConfiguration::decode(configuration).map_err(|_| Error)?;
        let Configuration::V1 { policy: p, .. } = bundle;
        let allowed = allowed_regimes
            .iter()
            .map(|r| match r {
                Regime::Falling => context::Regime::Falling,
                Regime::Flat => context::Regime::Flat,
                Regime::Rising => context::Regime::Rising,
            })
            .collect::<Vec<_>>();
        let allowed_text = allowed
            .iter()
            .map(|r| regime(*r))
            .collect::<Vec<_>>()
            .join(",");
        let rules = context::Rules::new(
            p.event_before_ms,
            p.event_after_ms,
            i64::from(context_max_age_ms),
            allowed.clone(),
        )
        .map_err(|_| Error)?;
        let window_start = decision
            .request
            .decision_at_ms
            .checked_sub(i64::from(p.event_before_ms))
            .ok_or(Error)?;
        let window_end = decision
            .request
            .decision_at_ms
            .checked_add(i64::from(p.event_after_ms))
            .ok_or(Error)?;
        let eligible = input
            .loaded
            .policy
            .assess(&input.loaded.series)
            .map_err(|_| Error)?;
        let mut text = format!(
            "{REVISION} artifact={} decision={} configuration={} policy={}\n",
            decision.outcome,
            decision.digest().map_err(|_| Error)?,
            decision.request.configuration,
            p.revision
        );
        writeln!(text,"direction=long_only horizon_maximum_inclusive_sessions={} base_age_maximum_inclusive_ms={} context_age_maximum_inclusive_ms={context_max_age_ms}",p.max_horizon_sessions,p.max_age_ms).map_err(|_|Error)?;
        for series in eligible.series() {
            let frozen = series.frozen();
            let id = frozen.selected.instrument;
            let source = hex(&frozen.evidence);
            let volume = input
                .volumes
                .iter()
                .find(|v| v.instrument == id)
                .ok_or(Error)?;
            let open = input
                .opens
                .iter()
                .find(|v| v.instrument == id)
                .ok_or(Error)?;
            let events = input
                .context
                .iter()
                .find(|v| v.instrument == id)
                .ok_or(Error)?;
            let liquidity = liquidity::calculate(series, volume).map_err(|_| Error)?;
            let market = market_constraints::calculate(series, open).map_err(|_| Error)?;
            let facts = rules
                .assess(series, events, decision.request.decision_at_ms)
                .map_err(|_| Error)?;
            let context::Coverage::Complete {
                source: event_source,
                known_at_ms,
                interval,
                events,
            } = &events.coverage
            else {
                return Err(Error);
            };
            writeln!(text,"instrument={:032x} evidence={source} selected_sessions={} calendar={} mapping={} basis=raw_usd",id.get(),series.closes().len(),hex(&frozen.calendar),hex(&frozen.selected.mapping)).map_err(|_|Error)?;
            writeln!(text,"evidence={source} revision={} turnover_cents={} minimum_inclusive={} below_minimum={} proxy=mean_close_times_reported_shares_floor_once",liquidity::REVISION,liquidity.cents(),p.min_daily_turnover_cents,liquidity.cents()<p.min_daily_turnover_cents).map_err(|_|Error)?;
            writeln!(
                text,
                "evidence={source} revision={} volatility_ppm={} maximum_inclusive={} exceeded={}",
                market_constraints::VOLATILITY_REVISION,
                market.volatility_ppm(),
                p.max_volatility_ppm,
                market.volatility_ppm() > p.max_volatility_ppm
            )
            .map_err(|_| Error)?;
            writeln!(
                text,
                "evidence={source} revision={} gap_ppm={} maximum_inclusive={} exceeded={}",
                market_constraints::GAP_REVISION,
                market.gap_ppm(),
                p.max_gap_ppm,
                market.gap_ppm() > p.max_gap_ppm
            )
            .map_err(|_| Error)?;
            writeln!(
                text,
                "evidence={source} revision={} regime={} allowed={allowed_text} denied={}",
                context::REGIME_REVISION,
                regime(facts.regime()),
                !allowed.contains(&facts.regime())
            )
            .map_err(|_| Error)?;
            writeln!(text,"evidence={source} event_source={} revision={} window_start_ms={window_start} window_end_ms={window_end} endpoints=inclusive coverage=complete coverage_start_ms={} coverage_end_exclusive_ms={} events_count={} conflict={}",hex(event_source),context::EVENT_REVISION,interval[0],interval[1],events.len(),facts.event_conflict()).map_err(|_|Error)?;
            for event in events {
                writeln!(text,"evidence={source} event_source={} calendar_event_at_ms={event} within_closed_window={}",hex(event_source),(*event>=window_start && *event<=window_end)).map_err(|_|Error)?;
            }
            let last = series.closes().last().ok_or(Error)?;
            writeln!(text,"evidence={source} event_source={} revision={} market_age_ms={} context_age_ms={} maximum_inclusive_ms={context_max_age_ms}",hex(event_source),context::QUALITY_REVISION,decision.request.decision_at_ms-last.event_at_ms,decision.request.decision_at_ms-known_at_ms).map_err(|_|Error)?;
            for (index, close) in series.closes().iter().enumerate() {
                writeln!(text,"evidence={source} selected_session_index={index} event_at_ms={} observed_at_ms={} available_at_ms={}",close.event_at_ms,close.observed_at_ms,close.available_at_ms).map_err(|_|Error)?;
            }
        }
        Ok(Self {
            base,
            measurements: text,
        })
    }
    /// Complete deterministic standard presentation with all required disclosures.
    /// # Errors
    /// Refuses an unrenderable standard proposal.
    pub fn standard(&self) -> Result<String, Error> {
        Ok(self.compose(&self.base.standard()?))
    }
    /// Validate typed choices and reproduce the exact preview without another inference.
    /// # Errors
    /// Refuses invalid claims, citations, coverage or presentation choices.
    pub fn validate(&self, proposal: &[u8]) -> Result<String, Error> {
        Ok(self.compose(&self.base.validate(proposal)?))
    }
    fn compose(&self, base: &str) -> String {
        let mut text = self.measurements.clone();
        // v2 is immutable: replace its header and unavailable source-summary rows
        // with the reconciled, explicitly source-bound chronology above.
        for line in base
            .lines()
            .skip(1)
            .filter(|line| !line.starts_with("instrument="))
        {
            text.push_str(line);
            text.push('\n');
        }
        text
    }
}
