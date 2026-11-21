//! Development-only summary over the same reconciled policy facts as the trace.
use crate::explanation::Error;
use edgeagent_contracts::research::ResearchRecord;
use edgeagent_research_domain::FrozenClose;
use std::fmt::Write as _;

pub(super) struct ReaderFirst {
    leader: String,
    policy: String,
    source: Option<String>,
}

impl ReaderFirst {
    pub(super) fn for_development(record: &ResearchRecord) -> Result<Option<Self>, Error> {
        let ResearchRecord::Artifact(artifact) = record else {
            return Ok(None);
        };
        if artifact.request.scope != "presentation-development" {
            return Ok(None);
        }
        Ok(Some(Self {
            leader: artifact.ranking.first().ok_or(Error)?.instrument.clone(),
            policy: String::new(),
            source: None,
        }))
    }

    pub(super) fn observe(
        &mut self,
        instrument: u128,
        source: &str,
        turnover_cents: u64,
        floor_cents: u64,
        last: &FrozenClose,
    ) -> Result<(), Error> {
        if turnover_cents < floor_cents {
            writeln!(
                self.policy,
                "Instrument {instrument:032x} is below the liquidity floor: {turnover_cents} USD cents observed; inclusive minimum {floor_cents} USD cents.\nEvidence: {source}"
            )
            .map_err(|_| Error)?;
        }
        let instrument = format!("{instrument:032x}");
        if instrument == self.leader {
            self.source = Some(format!(
                "Leading instrument {instrument}, latest selected close\nEvidence: {source}\nEvent time: {} ms. Observation time: {} ms. Availability time: {} ms.\n",
                last.event_at_ms, last.observed_at_ms, last.available_at_ms
            ));
        }
        Ok(())
    }

    pub(super) fn summary(&self) -> Result<String, Error> {
        let policy = if self.policy.is_empty() {
            "No instrument is below the liquidity floor.\n"
        } else {
            &self.policy
        };
        let source = self.source.as_deref().ok_or(Error)?;
        Ok(format!(
            "research-explanation-reader-first.v1\n\nLiquidity policy\n{policy}\nSource and timing\n{source}"
        ))
    }
}
