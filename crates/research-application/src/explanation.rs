//! Deterministic explanation preview for independently verified Research inputs.
use edgeagent_contracts::{
    explanation::{Anchor, Claim, Disclosure, Proposal, Selection, Variant},
    research::{EvidenceRef, RankedCandidate, ResearchRecord},
    research_decision::{Assessment, DecisionProvenance, Reason},
};
use std::fmt::{self, Write as _};

/// Payload-safe refusal of inconsistent inputs or an unsupported proposal.
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
    anchor: Anchor,
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
            anchor: Anchor {
                scope: artifact.request.scope.clone(),
                artifact: decision.outcome.clone(),
                decision: decision.digest().map_err(|_| Error)?,
            },
            ranks,
            reasons,
        })
    }

    /// Decode an untrusted proposal, verify all claims, and render a sealed result.
    ///
    /// The caller associates the model response with this allowlist. This checks
    /// factual support and presentation choices. Current disclosure authority is
    /// the responsibility of the calling boundary.
    ///
    /// # Errors
    /// Refuses invalid wire input, missing or reordered claims, unsupported
    /// citations, and variants outside the approved templates.
    pub fn validate(&self, bytes: &[u8]) -> Result<ValidatedExplanation, Error> {
        let proposal = Proposal::decode(bytes).map_err(|_| Error)?;
        let (ranks, tail) = proposal
            .claims
            .split_at_checked(self.ranks.len())
            .ok_or(Error)?;
        let (reasons, tail) = tail.split_at_checked(self.reasons.len()).ok_or(Error)?;
        let [disclosure] = tail else {
            return Err(Error);
        };
        let ranks_match = self.ranks.iter().zip(ranks).all(|(rank, selection)| {
            matches!(&selection.claim, Claim::Ranked { instrument }
                if instrument == &rank.candidate.instrument)
                && rank
                    .evidence
                    .iter()
                    .any(|reference| reference.digest == selection.citation)
        });
        let reasons_match = self.reasons.iter().zip(reasons).all(|(reason, selection)| {
            matches!(selection.claim, Claim::PolicyReason { reason: actual } if actual == *reason)
                && selection.citation == self.anchor.decision
                && selection.variant == Variant::Standard
        });
        if !ranks_match
            || !reasons_match
            || disclosure.claim
                != (Claim::Disclosure {
                    disclosure: Disclosure::ResearchOnly,
                })
            || disclosure.citation != self.anchor.artifact
            || disclosure.variant != Variant::Standard
        {
            return Err(Error);
        }
        let rendered = self.render_ranks(ranks).map_err(|_| Error)?;
        Ok(ValidatedExplanation {
            anchor: self.anchor.clone(),
            proposal,
            rendered,
        })
    }

    /// Render the preview in artifact rank order and retained policy reason order.
    ///
    /// # Errors
    /// Returns a formatting error if the output cannot be constructed.
    pub fn render(&self) -> Result<String, fmt::Error> {
        self.render_ranks(&[])
    }

    fn render_ranks(&self, selections: &[Selection]) -> Result<String, fmt::Error> {
        let mut text = String::new();
        writeln!(
            text,
            "research-explanation-trace.v1 artifact={} decision={}",
            self.anchor.artifact, self.anchor.decision
        )?;
        for (index, rank) in self.ranks.iter().enumerate() {
            match selections
                .get(index)
                .map_or(Variant::Standard, |value| value.variant)
            {
                Variant::PlainLanguage => write!(
                    text,
                    "Rank {}: {} scored {} parts per million.",
                    index + 1,
                    rank.candidate.instrument,
                    rank.candidate.score_ppm
                )?,
                Variant::Standard => write!(
                    text,
                    "rank={} instrument={} score_ppm={}",
                    index + 1,
                    rank.candidate.instrument,
                    rank.candidate.score_ppm
                )?,
            }
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
                self.anchor.decision
            )?;
        }
        text.push_str("Research only. This explanation confers no trading authority.\n");
        Ok(text)
    }
}

/// Validated proposal and deterministic rendering bound to exact Research inputs.
pub struct ValidatedExplanation {
    anchor: Anchor,
    proposal: Proposal,
    rendered: String,
}

impl ValidatedExplanation {
    /// Exact artifact, decision and scope validated by the allowlist.
    pub const fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    /// Accepted typed choices, retained for deterministic replay.
    pub const fn proposal(&self) -> &Proposal {
        &self.proposal
    }

    /// Presentation built exclusively from allowlisted facts and fixed templates.
    pub fn rendered(&self) -> &str {
        &self.rendered
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
