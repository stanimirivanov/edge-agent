//! Offline evaluation of invented explanation cases with independent ratings.
pub(crate) mod complete;
mod fixtures;
mod safety;

use crate::explanation::Allowlist;
use edgeagent_contracts::{
    evidence::DailyEvidenceArtifact, research::ResearchRecord,
    research_decision::DecisionProvenance,
};
use std::{collections::BTreeMap, fmt};

const PAIRS: usize = 30;
const MINIMUM_PREFERENCES: usize = 21;

fn identity(parts: &[&[u8]]) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part);
    }
    DailyEvidenceArtifact::hash(&bytes).as_str().to_owned()
}

/// Payload-safe error for unusable evaluation evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationError {
    /// A checked-in fixture does not match its frozen specification.
    Fixture,
    /// A deployment identity is absent or malformed.
    Deployment,
    /// Submitted case identities or independent ratings do not match the expected census.
    Census,
}

impl fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Fixture => "frozen explanation fixture is invalid",
            Self::Deployment => "evaluation deployment identity is invalid",
            Self::Census => "explanation evaluation census does not match",
        })
    }
}
impl std::error::Error for EvaluationError {}

/// One frozen scenario and the reader task that determines the comparison.
pub struct Case {
    id: &'static str,
    scenario: &'static str,
    task: &'static str,
    input: Option<Input>,
}

impl Case {
    /// Stable case identifier within the suite.
    pub const fn id(&self) -> &'static str {
        self.id
    }
    /// Description of the invented scenario.
    pub const fn scenario(&self) -> &'static str {
        self.scenario
    }
    /// Reader task for both sides of the comparison.
    pub const fn task(&self) -> &'static str {
        self.task
    }
    /// Exact fixture, absent until the renderer supports this task.
    pub const fn input(&self) -> Option<&Input> {
        self.input.as_ref()
    }
}

/// Frozen synthetic artifact, decision, and baseline for an executable case.
pub struct Input {
    identity: String,
    record: ResearchRecord,
    decision: DecisionProvenance,
    baseline: String,
    standard: Vec<u8>,
    complete_case: Option<usize>,
    prepared: Option<Vec<u8>>,
}

impl Input {
    /// Digest binding this case's inputs and baseline to its task.
    pub fn identity(&self) -> &str {
        &self.identity
    }
    /// Synthetic artifact to use for the case.
    pub const fn artifact(&self) -> &ResearchRecord {
        &self.record
    }
    /// Synthetic decision to use for the case.
    pub const fn decision(&self) -> &DecisionProvenance {
        &self.decision
    }
    /// Fixed baseline text presented to reviewers.
    pub fn baseline(&self) -> &str {
        &self.baseline
    }
    /// Canonical standard proposal for the positive validator control.
    pub fn standard_proposal(&self) -> &[u8] {
        &self.standard
    }

    /// Exact model preparation for the complete revision; absent for historical v1.
    pub fn prepared_input(&self) -> Option<&[u8]> {
        self.prepared.as_deref()
    }

    /// Independently validate and render a proposed candidate for blinded review.
    ///
    /// # Errors
    /// Refuses unsupported proposals and invalid fixture bindings.
    pub fn render_candidate(&self, bytes: &[u8]) -> Result<String, crate::explanation::Error> {
        if let Some(n) = self.complete_case {
            return complete::render(n, bytes);
        }
        let allowlist = Allowlist::from_verified(&self.record, &self.decision)?;
        Ok(allowlist.validate(bytes)?.rendered().to_owned())
    }

    /// Bind a rating to the exact outputs, deployment and presentation order.
    pub fn presentation_id(
        &self,
        deployment: &str,
        candidate: Candidate<'_>,
        candidate_first: bool,
    ) -> String {
        let (outcome, rendered) = self.observe(candidate);
        self.presentation(deployment, outcome, rendered.as_deref(), candidate_first)
    }

    fn presentation(
        &self,
        deployment: &str,
        outcome: Outcome,
        rendered: Option<&str>,
        candidate_first: bool,
    ) -> String {
        let kind = match outcome {
            Outcome::Generated => b"generated".as_slice(),
            Outcome::Fallback => b"fallback",
            Outcome::Failed => b"failed",
            Outcome::Rejected => b"rejected",
        };
        identity(&[
            self.identity.as_bytes(),
            deployment.as_bytes(),
            kind,
            rendered.unwrap_or_default().as_bytes(),
            &[u8::from(candidate_first)],
        ])
    }

    fn observe(&self, candidate: Candidate<'_>) -> (Outcome, Option<String>) {
        match candidate {
            Candidate::Proposed(bytes) => match self.render_candidate(bytes) {
                Ok(text) => (Outcome::Generated, Some(identity(&[text.as_bytes()]))),
                Err(_) => (Outcome::Rejected, None),
            },
            Candidate::Fallback => (Outcome::Fallback, None),
            Candidate::Failed => (Outcome::Failed, None),
        }
    }
}

/// Candidate execution observation for one frozen pair.
#[derive(Clone, Copy)]
pub enum Candidate<'a> {
    /// Raw typed proposal for independent validation.
    Proposed(&'a [u8]),
    /// Deterministic fallback was used.
    Fallback,
    /// The candidate did not produce an output.
    Failed,
}

/// One candidate run and recorded blind presentation order.
#[derive(Clone, Copy)]
pub struct CaseResult<'a> {
    /// Frozen case identifier.
    pub id: &'a str,
    /// Frozen input identity.
    pub input: &'a str,
    /// Whether the candidate was presented before the baseline.
    pub candidate_first: bool,
    /// Execution observation.
    pub candidate: Candidate<'a>,
}

/// Preference recorded without exposing the candidate's position to the reviewer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Preference {
    /// First displayed explanation.
    First,
    /// Second displayed explanation.
    Second,
    /// Neither explanation is preferred.
    Tie,
}

/// One blinded response.
#[derive(Clone, Copy)]
pub struct Rating<'a> {
    /// Frozen case identifier.
    pub id: &'a str,
    /// Exact displayed pair identity from the frozen input.
    pub presentation: &'a str,
    /// Preferred position or tie.
    pub preference: Preference,
}

/// Independently supplied ratings from one identified reviewer.
#[derive(Clone, Copy)]
pub struct Review<'a> {
    /// Externally authenticated reviewer identifier.
    pub reviewer: &'a str,
    /// Exactly one rating per available pair.
    pub ratings: &'a [Rating<'a>],
}

/// Qualification scoring result. Runtime activation remains separate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Disposition {
    /// Some frozen tasks cannot yet be run.
    Incomplete,
    /// The complete suite missed a required threshold, or a safety check failed.
    Failed,
    /// The complete suite met both independent thresholds and all safety checks.
    Passed,
}

/// Independently classified candidate output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// A validated proposal and deterministic rendering.
    Generated,
    /// A deterministic fallback.
    Fallback,
    /// No candidate output.
    Failed,
    /// The evaluator refused the candidate's proposed output.
    Rejected,
}

/// Auditable result for one executable pair, without raw rejected data.
pub struct Pair<'a> {
    /// Exact frozen case identity.
    pub id: &'static str,
    /// Digest of the frozen artifact, decision, task and baseline.
    pub input: &'a str,
    /// Recorded candidate position.
    pub candidate_first: bool,
    /// Exact pair identity used by both reviewers.
    pub presentation: String,
    /// Independently classified outcome.
    pub outcome: Outcome,
    /// Digest of accepted rendered candidate output, if any.
    pub rendered: Option<String>,
    /// Ratings in the report's reviewer order.
    pub ratings: [Preference; 2],
}

/// Result of one frozen validator or authorization probe.
pub struct SafetyCheck {
    /// Stable probe identifier.
    pub id: &'static str,
    /// Whether the actual disposition matched the frozen expectation.
    pub passed: bool,
}

/// Evaluation evidence bound to one suite and deployment.
pub struct Report<'a> {
    /// Frozen suite identity including baseline and rubric revisions.
    pub suite: &'a str,
    /// Independently pinned deployment identity supplied by the runner.
    pub deployment: &'a str,
    /// Reviewer identifiers in scoring order.
    pub reviewers: [&'a str; 2],
    /// Case results in frozen order.
    pub pairs: Vec<Pair<'a>>,
    /// Candidate preferences for each reviewer, out of 30.
    pub preferences: [usize; 2],
    /// Frozen tasks awaiting compatible fixtures.
    pub unresolved: Vec<&'static str>,
    /// Fixed safety probes and their results.
    pub safety: Vec<SafetyCheck>,
    /// Overall result.
    pub disposition: Disposition,
}

/// Checked-in evaluation census. Callers cannot substitute an easier suite.
pub struct Suite {
    cases: Vec<Case>,
    identity: String,
    complete: bool,
}

impl Suite {
    /// Load the frozen synthetic fixtures and verify the baseline pins.
    ///
    /// # Errors
    /// Rejects drift in the checked-in fixture bytes or renderer.
    pub fn frozen() -> Result<Self, EvaluationError> {
        fixtures::load()
    }

    /// Load the complete semantically checked v2 census, preserving the historical v1 protocol.
    /// # Errors
    /// Refuses semantic drift, preparation/presentation drift, or changed frozen pins.
    pub fn complete() -> Result<Self, EvaluationError> {
        complete::load()
    }

    /// Canonical identity of this exact suite, rubric and baseline.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Frozen usefulness cases, including unresolved cases.
    pub fn cases(&self) -> &[Case] {
        &self.cases
    }

    /// Check a full executable census and score both independent reviewers.
    ///
    /// The runner verifies deployment identity and reviewer independence, records
    /// blind order, and submits all results. This evaluator makes no external calls.
    ///
    /// # Errors
    /// Rejects malformed deployment identity and incomplete or inconsistent submissions.
    pub fn evaluate<'a>(
        &'a self,
        deployment: &'a str,
        results: &[CaseResult<'_>],
        reviews: [Review<'a>; 2],
    ) -> Result<Report<'a>, EvaluationError> {
        if deployment.len() != 64
            || !deployment
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || deployment.bytes().all(|b| b == b'0')
        {
            return Err(EvaluationError::Deployment);
        }
        let ready = self
            .cases
            .iter()
            .filter(|case| case.input.is_some())
            .count();
        if results.len() != ready
            || reviews[0].reviewer == reviews[1].reviewer
            || reviews
                .iter()
                .any(|review| review.ratings.len() != ready || !valid_reviewer(review.reviewer))
        {
            return Err(EvaluationError::Census);
        }
        let mut results: BTreeMap<_, _> =
            results.iter().map(|result| (result.id, result)).collect();
        let mut ratings = reviews.map(|review| {
            review
                .ratings
                .iter()
                .map(|rating| (rating.id, rating))
                .collect::<BTreeMap<_, _>>()
        });
        let mut pairs = Vec::with_capacity(ready);
        let mut unresolved = Vec::new();
        for case in &self.cases {
            let Some(input) = &case.input else {
                unresolved.push(case.id);
                continue;
            };
            let result = results
                .remove(case.id)
                .filter(|result| result.input == input.identity)
                .ok_or(EvaluationError::Census)?;
            let (outcome, rendered) = input.observe(result.candidate);
            let presentation = input.presentation(
                deployment,
                outcome,
                rendered.as_deref(),
                result.candidate_first,
            );
            let preference = |ratings: &mut BTreeMap<_, &Rating<'_>>| {
                ratings
                    .remove(case.id)
                    .filter(|rating| rating.presentation == presentation)
                    .map(|rating| rating.preference)
                    .ok_or(EvaluationError::Census)
            };
            let preferences = [preference(&mut ratings[0])?, preference(&mut ratings[1])?];
            pairs.push(Pair {
                id: case.id,
                input: input.identity(),
                candidate_first: result.candidate_first,
                outcome,
                presentation,
                rendered,
                ratings: preferences,
            });
        }
        let safety = if self.complete {
            complete::checks(self)?
        } else {
            safety::run()?
        };
        let safe = safety.iter().all(|check| check.passed);
        let (preferences, disposition) = score(&pairs, safe);
        Ok(Report {
            suite: &self.identity,
            deployment,
            reviewers: reviews.map(|review| review.reviewer),
            pairs,
            preferences,
            unresolved,
            safety,
            disposition,
        })
    }
}

fn valid_reviewer(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn score(pairs: &[Pair<'_>], safe: bool) -> ([usize; 2], Disposition) {
    let preferences = [0, 1].map(|reviewer| {
        pairs
            .iter()
            .filter(|pair| {
                let candidate = if pair.candidate_first {
                    Preference::First
                } else {
                    Preference::Second
                };
                pair.outcome == Outcome::Generated && pair.ratings[reviewer] == candidate
            })
            .count()
    });
    let disposition = if !safe {
        Disposition::Failed
    } else if pairs.len() != PAIRS {
        Disposition::Incomplete
    } else if preferences
        .iter()
        .all(|count| *count >= MINIMUM_PREFERENCES)
    {
        Disposition::Passed
    } else {
        Disposition::Failed
    };
    (preferences, disposition)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_reviewers_need_twenty_one_of_the_same_thirty_pairs() {
        let mut pairs: Vec<_> = (0..PAIRS)
            .map(|index| {
                let candidate_first = index % 2 == 0;
                let preferred = if candidate_first {
                    Preference::First
                } else {
                    Preference::Second
                };
                Pair {
                    id: "test",
                    input: "test",
                    candidate_first,
                    outcome: Outcome::Generated,
                    presentation: String::new(),
                    rendered: None,
                    ratings: if index < 21 {
                        [preferred; 2]
                    } else {
                        [Preference::Tie; 2]
                    },
                }
            })
            .collect();
        assert_eq!(score(&pairs, true), ([21, 21], Disposition::Passed));
        assert_eq!(score(&pairs, false), ([21, 21], Disposition::Failed));
        pairs[0].ratings[1] = Preference::Tie;
        assert_eq!(score(&pairs, true), ([21, 20], Disposition::Failed));
        pairs[1].outcome = Outcome::Fallback;
        pairs[2].outcome = Outcome::Failed;
        pairs[3].outcome = Outcome::Rejected;
        assert_eq!(score(&pairs, true), ([18, 17], Disposition::Failed));
        assert_eq!(
            score(&pairs[..29], true),
            ([18, 17], Disposition::Incomplete)
        );
    }
}
