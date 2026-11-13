//! Semantic guards run before pins: a matching digest cannot make the wrong scenario valid.
use super::*;
use edgeagent_contracts::research_decision::{Assessment, Direction, Reason, Status};
pub(super) fn check(n: usize, f: &recipe::Fixture, text: &str) -> Result<(), EvaluationError> {
    let ResearchRecord::Artifact(a) = &f.record else {
        return Err(INVALID);
    };
    for key in ["prices", "opens"] {
        let rows = f.assertions[key].as_array().ok_or(INVALID)?;
        if rows.len() != a.request.instruments.len()
            || rows.iter().any(|r| {
                r.as_array()
                    .is_none_or(|v| v.len() != usize::from(a.request.lookback_sessions) + 1)
            })
        {
            return Err(INVALID);
        }
    }
    let Assessment::Evaluated {
        status, reasons, ..
    } = &f.decision.assessment
    else {
        return Err(INVALID);
    };
    let scores: Vec<_> = a.ranking.iter().map(|r| r.score_ppm).collect();
    let line = |source: u8, fragment: &str| {
        text.lines()
            .any(|l| l.contains(&format!("{source:02x}").repeat(32)) && l.contains(fragment))
    };
    let valid = match n {
        1 => a.ranking.len() == 1 && reasons.is_empty() && text.contains("below_minimum=false"),
        2 => scores.len() == 2 && scores[0] > scores[1],
        3 => scores == [20001, 20000, 5000],
        4 => {
            scores.len() == 2
                && scores[0] == scores[1]
                && a.ranking[0].instrument < a.ranking[1].instrument
                && text.contains("ties=instrument_ascending")
        }
        5 => scores.len() == 2 && scores.iter().all(|s| *s < 0),
        6 => scores == [10000, 0, -10000],
        7 => {
            a.ranking.len() == 2
                && a.ranking[0].instrument[..31] == a.ranking[1].instrument[..31]
                && a.ranking[0].instrument != a.ranking[1].instrument
        }
        8 => {
            reasons == &[Reason::LiquidityBelowMinimum]
                && line(6, "turnover_cents=0 minimum_inclusive=1 below_minimum=true")
        }
        9 => {
            *status == Status::Approved
                && line(
                    5,
                    "turnover_cents=1000000 minimum_inclusive=1000000 below_minimum=false",
                )
        }
        10 => {
            reasons == &[Reason::VolatilityExceeded]
                && line(6, "volatility_ppm=95455 maximum_inclusive=0 exceeded=true")
        }
        11 => {
            reasons == &[Reason::GapExceeded]
                && line(6, "gap_ppm=100000 maximum_inclusive=0 exceeded=true")
                && line(6, "volatility_ppm=0")
        }
        12 => {
            reasons == &[Reason::VolatilityExceeded, Reason::GapExceeded]
                && line(6, "volatility_ppm=95455")
                && line(6, "gap_ppm=100000")
        }
        13 => {
            *status == Status::Approved
                && f.decision.direction == Direction::Long
                && text.contains("direction=long_only")
                && text.contains("no trading authority")
        }
        14 => {
            f.decision.horizon_sessions == 3
                && text.contains("horizon_maximum_inclusive_sessions=3")
        }
        15 => {
            a.request.instruments.len() == 1
                && a.ranking.len() == 1
                && text.contains(&format!("universe={}", a.request.instruments[0]))
        }
        16 => {
            reasons == &[Reason::RegimeNotAllowed]
                && line(6, "regime=falling allowed=flat,rising denied=true")
        }
        17 => {
            *status == Status::Approved
                && scores == [0]
                && line(5, "regime=flat allowed=flat denied=false")
        }
        18 | 19 => {
            reasons == &[Reason::EventExcluded]
                && line(
                    6,
                    &format!(
                        "calendar_event_at_ms={} within_closed_window=true",
                        if n == 18 { 95 } else { 105 }
                    ),
                )
                && text.contains("window_start_ms=95 window_end_ms=105 endpoints=inclusive")
        }
        20 => {
            *status == Status::Approved
                && line(5, "events_count=0 conflict=false")
                && text.contains("coverage=complete")
        }
        21 => {
            line(
                5,
                "market_age_ms=9 context_age_ms=9 maximum_inclusive_ms=10",
            ) && line(
                6,
                "market_age_ms=10 context_age_ms=10 maximum_inclusive_ms=10",
            )
        }
        22 => {
            *status == Status::Approved
                && line(
                    5,
                    "market_age_ms=10 context_age_ms=10 maximum_inclusive_ms=10",
                )
        }
        23 => line(5, "event_at_ms=90 observed_at_ms=95 available_at_ms=100"),
        24 => {
            a.evidence.len() == 2
                && a.evidence[0].digest != a.evidence[1].digest
                && line(5, "turnover_cents=")
                && line(6, "turnover_cents=")
        }
        25 => ["turnover_cents=", "volatility_ppm=", "gap_ppm="]
            .iter()
            .all(|v| line(5, v)),
        26 => {
            reasons == &[Reason::LiquidityBelowMinimum, Reason::GapExceeded]
                && line(6, "below_minimum=true")
                && line(7, "gap_ppm=100000 maximum_inclusive=0 exceeded=true")
                && line(6, "gap_ppm=0 maximum_inclusive=0 exceeded=false")
        }
        27 => a.expires_at_ms == 101 && text.contains("artifact_expires_at_ms=101"),
        28 => {
            a.request.lookback_sessions == 1
                && f.decision.horizon_sessions == 3
                && text.contains("holding_horizon_sessions=3 measurement_lookback_sessions=1")
        }
        29 => {
            text.contains("invented-reviewed-release")
                && text.contains("context only; not evidence for ranking or policy")
                && text.contains("context_digest=")
        }
        30 => [
            "Research only.",
            "not a current-market quote.",
            "Scenarios and risk illustrations: not qualified.",
        ]
        .iter()
        .all(|s| text.contains(s)),
        _ => false,
    };
    if valid { Ok(()) } else { Err(INVALID) }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjacent_but_different_scenarios_do_not_satisfy_semantic_guards()
    -> Result<(), EvaluationError> {
        for (actual, claimed) in [(2, 4), (2, 5), (12, 26)] {
            let (fixture, text, _, _) = materialize(actual, None)?;
            assert!(check(claimed, &fixture, &text).is_err());
        }
        let fixture = recipe::build(25)?;
        let old = Allowlist::from_verified(&fixture.record, &fixture.decision)
            .map_err(|_| INVALID)?
            .render()
            .map_err(|_| INVALID)?;
        assert!(check(25, &fixture, &old).is_err());
        let (mut short, text, _, _) = materialize(28, None)?;
        short.assertions["prices"][0]
            .as_array_mut()
            .ok_or(INVALID)?
            .push(999.into());
        assert!(check(28, &short, &text).is_err());
        Ok(())
    }
}
