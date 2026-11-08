//! Deterministic explanation preview for independently verified Research inputs.
use edgeagent_contracts::{
    research::{EvidenceRef, RankedCandidate, ResearchRecord},
    research_decision::{Assessment, DecisionProvenance, Reason},
};
use std::fmt::{self, Write as _};

/// Payload-safe refusal to explain inconsistent or incomplete inputs.
#[derive(Debug)]
pub struct Error;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("explanation inputs refused")
    }
}
impl std::error::Error for Error {}

struct Rank<'a> {
    candidate: &'a RankedCandidate,
    evidence: Vec<&'a EvidenceRef>,
}

/// Closed set of facts eligible for the local deterministic preview.
pub struct Allowlist<'a> {
    artifact: String,
    decision: String,
    ranks: Vec<Rank<'a>>,
    reasons: &'a [Reason],
}

impl<'a> Allowlist<'a> {
    /// Select exact ranks, supporting evidence references, and retained policy reasons.
    /// The caller must authenticate the retained completion and independently verify
    /// the artifact and decision with currently authorized owner evidence.
    ///
    /// # Errors
    /// Refuses non-artifacts, mismatched decisions, and ranks without evidence.
    pub fn from_verified(
        record: &'a ResearchRecord,
        decision: &'a DecisionProvenance,
    ) -> Result<Self, Error> {
        decision.bind(record).map_err(|_| Error)?;
        let (ResearchRecord::Artifact(artifact), Assessment::Evaluated { reasons, .. }) =
            (record, &decision.assessment)
        else {
            return Err(Error);
        };
        let mut ranks = Vec::with_capacity(artifact.ranking.len());
        for candidate in &artifact.ranking {
            let evidence: Vec<_> = artifact
                .evidence
                .iter()
                .filter(|entry| entry.instrument == candidate.instrument)
                .collect();
            if evidence.is_empty() {
                return Err(Error);
            }
            ranks.push(Rank {
                candidate,
                evidence,
            });
        }
        Ok(Self {
            artifact: decision.outcome.clone(),
            decision: decision.digest().map_err(|_| Error)?,
            ranks,
            reasons,
        })
    }

    /// Render the preview in artifact rank order and retained policy reason order.
    ///
    /// # Errors
    /// Returns a formatting error if the output cannot be constructed.
    pub fn render(&self) -> Result<String, fmt::Error> {
        let mut text = String::new();
        writeln!(
            text,
            "research-explanation-trace.v1 artifact={} decision={}",
            self.artifact, self.decision
        )?;
        for (index, rank) in self.ranks.iter().enumerate() {
            write!(
                text,
                "rank={} instrument={} score_ppm={}",
                index + 1,
                rank.candidate.instrument,
                rank.candidate.score_ppm
            )?;
            for evidence in &rank.evidence {
                write!(
                    text,
                    " evidence={} available_at_ms={}",
                    evidence.digest, evidence.available_at_ms
                )?;
            }
            text.push('\n');
        }
        for reason in self.reasons {
            writeln!(
                text,
                "policy_reason={} decision={}",
                reason_code(*reason),
                self.decision
            )?;
        }
        text.push_str("Research only. This explanation confers no trading authority.\n");
        Ok(text)
    }
}

const fn reason_code(reason: Reason) -> &'static str {
    match reason {
        Reason::InstrumentNotAllowed => "instrument_not_allowed",
        Reason::DirectionNotAllowed => "direction_not_allowed",
        Reason::HorizonExceeded => "horizon_exceeded",
        Reason::LiquidityBelowMinimum => "liquidity_below_minimum",
        Reason::VolatilityExceeded => "volatility_exceeded",
        Reason::GapExceeded => "gap_exceeded",
        Reason::EventExcluded => "event_excluded",
        Reason::RegimeNotAllowed => "regime_not_allowed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgeagent_contracts::research_decision::Status;
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn preview_uses_exact_rank_and_reason_order_with_scoped_citations() -> TestResult {
        let mut record = ResearchRecord::decode(include_bytes!(
            "../../contracts/fixtures/research/v1/artifact.json"
        ))?;
        let mut decision = DecisionProvenance::decode(include_bytes!(
            "../../contracts/fixtures/research/decision-v1.json"
        ))?;
        let ResearchRecord::Artifact(artifact) = &mut record else {
            return Err("expected artifact".into());
        };
        artifact
            .request
            .instruments
            .extend(["BBB".into(), "CCC".into()]);
        artifact.ranking.insert(
            0,
            RankedCandidate {
                instrument: "BBB".into(),
                score_ppm: 20_000,
            },
        );
        artifact.evidence.extend([
            EvidenceRef {
                instrument: "BBB".into(),
                digest: "d".repeat(64),
                available_at_ms: 91,
            },
            EvidenceRef {
                instrument: "CCC".into(),
                digest: "e".repeat(64),
                available_at_ms: 92,
            },
        ]);
        artifact.explanation = "UNRENDERED_TEXT".into();
        decision.request = artifact.request.clone();
        decision.outcome = record.digest()?;
        decision.assessment = Assessment::Evaluated {
            status: Status::Narrowed,
            eligible: vec!["AAA".into(), "BBB".into()],
            reasons: vec![Reason::LiquidityBelowMinimum, Reason::EventExcluded],
        };
        let rendered = Allowlist::from_verified(&record, &decision)?.render()?;
        let digest = decision.digest()?;
        let expected = format!(
            "research-explanation-trace.v1 artifact={} decision={digest}\nrank=1 instrument=BBB score_ppm=20000 evidence={} available_at_ms=91\nrank=2 instrument=AAA score_ppm=10000 evidence={} available_at_ms=90\npolicy_reason=liquidity_below_minimum decision={digest}\npolicy_reason=event_excluded decision={digest}\nResearch only. This explanation confers no trading authority.\n",
            decision.outcome,
            "d".repeat(64),
            "c".repeat(64)
        );
        assert_eq!(rendered, expected);

        let ResearchRecord::Artifact(artifact) = &mut record else {
            return Err("expected artifact".into());
        };
        artifact.evidence.retain(|entry| entry.instrument != "BBB");
        assert!(Allowlist::from_verified(&record, &decision).is_err());
        Ok(())
    }
}
