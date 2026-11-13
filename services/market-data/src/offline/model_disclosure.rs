//! Owner-local model review/permit commands; no model invocation or content output.
use super::*;
use edgeagent_contracts::model_disclosure::{MAX_REQUEST_BYTES, Request};
use edgeagent_market_data_postgres::{ModelDisclosureReview, PostgresResearchEvidence};
use std::io::{Read as _, Write as _};

pub(super) async fn execute(
    config: &config::Config,
    bridge: &PostgresResearchEvidence,
    ledger: &PostgresAcquisitionLedger,
    storage: &impl EvidenceStorage,
) -> Result {
    let review = config.command == "offline-model-disclosure-review";
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(if review {
            10241
        } else {
            (MAX_REQUEST_BYTES + 1) as u64
        })
        .read_to_end(&mut bytes)?;
    let bind = |request: &Request| -> Result {
        if request.acquisition.as_str()
            != format!("{:032x}", config.request.ok_or("missing acquisition")?)
            || request.research.scope != std::env::var("EDGEAGENT_OFFLINE_RESEARCH_SCOPE")?
        {
            return Err(BudgetError::new(BudgetErrorKind::Denied).into());
        }
        Ok(())
    };
    if review {
        let command = ModelDisclosureReview::decode(&bytes)?;
        bind(command.request())?;
        bridge.review_model_disclosure(&command).await?;
        return Ok(());
    }
    let request = Request::decode(&bytes)?;
    bind(&request)?;
    let retained = bridge.model_disclosure_evidence(&request).await?;
    let job = Job::restore(&retained.admission)?;
    let access = Access {
        job: &job,
        captured: now()?,
    };
    let edgeagent_contracts::research_acquisition::ResearchEvidence::Ready { decision, .. } =
        edgeagent_contracts::research_acquisition::ResearchEvidence::from_envelope(
            retained.publication.as_ref().ok_or("missing publication")?,
        )?
    else {
        return Err(BudgetError::new(BudgetErrorKind::Denied).into());
    };
    let EvidenceOutcome::Ready { object } = &decision.outcome else {
        return Err(BudgetError::new(BudgetErrorKind::Denied).into());
    };
    RetainedEvidenceStorage::new(storage, ledger)
        .get(object, &access)
        .await?;
    let permit = bridge.model_disclosure_permit(&request, &access).await?;
    std::io::stdout().write_all(&permit.canonical_bytes()?)?;
    Ok(())
}
