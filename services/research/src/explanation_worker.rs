//! Finite execution of authenticated Research jobs. Current qualification keeps generation disabled.
use crate::{
    loader::{OwnerEndpoint, OwnerLoader},
    runtime::{Result, clock_ms},
};
use edgeagent_contracts::{
    explanation::{Fallback, Refusal, ServingPermit},
    explanation_profile::Profile,
};
use edgeagent_research_application::{
    explanation_context::{Grant, Purpose, Request},
    explanation_worker::Prepared,
    publication,
};
use edgeagent_research_postgres::{
    PostgresResearchStore,
    explanation_attempts::Recovery,
    explanation_completion::{Completion, Receipt},
};
use std::{io::Read as _, time::Duration};

/// Process one exact durable job. Internal recovery does not grant serving authority.
/// # Errors
/// Leaves unavailable or inconsistent jobs recoverable without partial completion.
pub async fn execute(
    store: &PostgresResearchStore,
    scope: &str,
    id: Option<&str>,
) -> Result<Option<Receipt>> {
    let Some(job) = store.explanation_job(scope, id).await? else {
        return Ok(None);
    };
    if job.receipt.is_some() {
        return Ok(job.receipt);
    }
    let recovery = store
        .recover_explanation_attempt(&job.job, || clock_ms().unwrap_or(-1))
        .await?;
    if matches!(recovery, Recovery::Pending { .. }) {
        return Ok(None);
    }
    if matches!(recovery, Recovery::Terminal) {
        return Ok(store.recover_explanation(&job.request, &job.job).await?);
    }
    let path = std::env::var("EDGEAGENT_RESEARCH_EXPLANATION_PROFILE")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(2049)
        .read_to_end(&mut bytes)?;
    let profile = Profile::decode(&bytes)?;
    let decision =
        publication::decode_decision(job.original.receipt.decision().ok_or("missing decision")?)?;
    let prepared = Prepared::new(job.original.receipt.record(), &decision, &profile)?;
    let reason = match &recovery {
        Recovery::Fallback(spent) => {
            let binding = &spent.intent().binding;
            if binding.anchor != prepared.binding().anchor
                || binding.profile != prepared.binding().profile
                || binding.evidence != prepared.binding().evidence
                || spent.intent().input_digest != prepared.input_digest()
            {
                return Err("pending explanation requires original profile and input".into());
            }
            Fallback::UncertainAttempt
        }
        _ => Fallback::Disabled,
    };
    let input = store.explanation_input(&job).await?;
    let serving = Request {
        anchor: job.job.anchor.clone(),
        audience: std::env::var("EDGEAGENT_RESEARCH_EXPLANATION_AUDIENCE")?,
        purpose: Purpose::ExplanationServing,
        document: None,
    };
    edgeagent_contracts::evidence::EvidenceId::new(&serving.audience)?;
    if !serving.is_valid() {
        return Err("invalid serving audience".into());
    }
    if store
        .explanation_grant(&serving, clock_ms()?)
        .await?
        .is_none()
    {
        return commit(
            store,
            &job.request,
            prepared.binding(),
            Completion::Refused(Refusal::DisclosureDenied),
            None,
        )
        .await;
    }
    let owner = OwnerEndpoint::from_environment()?;
    if let Err(reason) = authorize(&owner, &input, &serving).await {
        return commit(
            store,
            &job.request,
            prepared.binding(),
            Completion::Refused(reason),
            None,
        )
        .await;
    }
    let selection = job.original.selection.ok_or("missing retained selection")?;
    let loader = OwnerLoader::from_explanation_input(&selection.profile(scope), &input)?;
    let verified = tokio::time::timeout(
        Duration::from_secs(25),
        edgeagent_research_application::replay::verify_execution(
            &loader,
            &input,
            &job.original.receipt,
            selection,
            edgeagent_research_application::RENDERER_REVISION,
            edgeagent_research_domain::momentum::REVISION,
        ),
    )
    .await;
    if !matches!(verified, Ok(Ok(_))) {
        return commit(
            store,
            &job.request,
            prepared.binding(),
            Completion::Refused(Refusal::AuthorityUnavailable),
            None,
        )
        .await;
    }
    let grant = match authorize(&owner, &input, &serving).await {
        Ok(grant) => grant,
        Err(reason) => {
            return commit(
                store,
                &job.request,
                prepared.binding(),
                Completion::Refused(reason),
                None,
            )
            .await;
        }
    };
    // No configuration flag can enable an unqualified runtime. Model composition is tested independently.
    let rendered = prepared.fallback(reason)?;
    commit(
        store,
        &job.request,
        prepared.binding(),
        Completion::Fallback {
            explanation: &rendered.explanation,
            reason,
        },
        Some(&grant),
    )
    .await
}
async fn commit(
    store: &PostgresResearchStore,
    request: &edgeagent_contracts::research::ResearchRequest,
    binding: &edgeagent_contracts::explanation::Binding,
    completion: Completion<'_>,
    owner: Option<&Grant>,
) -> Result<Option<Receipt>> {
    let result = store
        .complete_explanation(
            request,
            binding,
            completion,
            owner,
            || clock_ms().unwrap_or(-1),
            &time()?,
        )
        .await;
    let result = match result {
        Err(error) if error.kind() == publication::PublicationErrorKind::Disabled => {
            store
                .complete_explanation(
                    request,
                    binding,
                    Completion::Refused(Refusal::DisclosureDenied),
                    None,
                    || clock_ms().unwrap_or(-1),
                    &time()?,
                )
                .await
        }
        other => other,
    };
    match result {
        Ok(receipt) => Ok(Some(receipt)),
        Err(error) if error.kind() == publication::PublicationErrorKind::Conflict => {
            let job = edgeagent_contracts::explanation::Job {
                anchor: binding.anchor.clone(),
            };
            Ok(Some(
                store
                    .recover_explanation(request, &job)
                    .await?
                    .ok_or("conflicting terminal explanation")?,
            ))
        }
        Err(error) => Err(error.into()),
    }
}
async fn authorize(
    owner: &OwnerEndpoint,
    event: &edgeagent_contracts::MessageEnvelope,
    serving: &Request,
) -> std::result::Result<Grant, Refusal> {
    match tokio::time::timeout(Duration::from_secs(5), owner_grant(owner, event, serving)).await {
        Ok(Ok(Some(grant))) => Ok(grant),
        Ok(Ok(None)) => Err(Refusal::DisclosureDenied),
        _ => Err(Refusal::AuthorityUnavailable),
    }
}

fn time() -> Result<String> {
    Ok(chrono::DateTime::from_timestamp_millis(clock_ms()?)
        .ok_or("invalid clock")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
async fn owner_grant(
    owner: &OwnerEndpoint,
    event: &edgeagent_contracts::MessageEnvelope,
    serving: &Request,
) -> Result<Option<Grant>> {
    use edgeagent_contracts::{evidence::EvidenceOutcome, research_acquisition::ResearchEvidence};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let ResearchEvidence::Ready { request, decision } = ResearchEvidence::from_envelope(event)?
    else {
        return Err("missing owner evidence".into());
    };
    let EvidenceOutcome::Ready { object } = &decision.outcome else {
        return Err("missing owner object".into());
    };
    let mut child = owner
        .command(&request.scope)
        .args([
            "offline-explanation-permit",
            decision.request.request.as_str(),
            &serving.audience,
        ])
        .spawn()?;
    let bytes = event.to_json()?;
    let mut stdin = child.stdin.take().ok_or("owner stdin")?;
    let stdout = child.stdout.take().ok_or("owner stdout")?;
    let reading = async {
        let mut bytes = Vec::new();
        stdout
            .take(2049)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    };
    let writing = async {
        stdin.write_all(&bytes).await?;
        drop(stdin);
        Ok::<_, std::io::Error>(())
    };
    let (read, written) = tokio::join!(reading, writing);
    let response = read?;
    let status = child.wait().await?;
    if status.code() == Some(77) {
        return Ok(None);
    }
    written?;
    if !status.success() || response.len() > 2048 {
        return Err("owner permit unavailable".into());
    }
    let permit: ServingPermit = serde_json::from_slice(&response)?;
    let now = clock_ms()?;
    let retained_until = object
        .delete_by
        .seconds
        .checked_mul(1000)
        .and_then(|ms| ms.checked_add(i64::from(object.delete_by.nanos / 1_000_000)))
        .ok_or("invalid owner retention cutoff")?;
    if permit.expires_at_ms > retained_until {
        return Err("owner permit exceeds retention".into());
    }
    if permit.scope != request.scope
        || permit.acquisition != decision.request.request.as_str()
        || permit.evidence != object.sha256.as_str()
        || permit.audience != serving.audience
        || permit.checked_at_ms < 0
        || permit.checked_at_ms > now
        || now >= permit.expires_at_ms
    {
        return Err("invalid owner permit".into());
    }
    Ok(Some(Grant {
        request: serving.clone(),
        valid_from_ms: permit.checked_at_ms,
        expires_at_ms: permit.expires_at_ms,
        revoked: false,
    }))
}
