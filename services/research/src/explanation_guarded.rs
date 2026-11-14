//! Opt-in owned execution. No synthetic verifier, arbitrary URL or enable flag.
use crate::{
    explanation_deployment::Verifier,
    explanation_model::HttpModel,
    explanation_owned::{Manifest, digest},
    explanation_worker::{authorize, commit},
    loader::{OwnerEndpoint, OwnerLoader},
    runtime::{Result, clock_ms},
};
use edgeagent_contracts::{
    evidence::{EvidenceId, EvidenceOutcome},
    explanation::{Fallback, Refusal},
    model_disclosure as disclosure,
    research_acquisition::ResearchEvidence,
};
use edgeagent_research_application::{
    explanation_context::{Purpose, Request},
    explanation_model::{ExplanationModel, Failure},
    explanation_runtime::RuntimePreparation,
    publication,
    worker::ExecutionLoader,
};
use edgeagent_research_postgres::{
    PostgresResearchStore,
    explanation_attempts::{Intent, QualifiedAuthority, Recovery, ReservationOutcome},
    explanation_completion::{Completion, Receipt},
};
use std::{
    io::Read as _,
    net::{Ipv4Addr, SocketAddrV4},
    time::Duration,
};

pub(crate) async fn execute(
    store: &PostgresResearchStore,
    scope: &str,
    id: Option<&str>,
) -> Result<Option<Receipt>> {
    let started = std::time::Instant::now();
    let result = execute_inner(store, scope, id).await;
    let outcome = match &result {
        Ok(Some(receipt)) => crate::explanation_worker::terminal_outcome(receipt),
        Ok(None) => edgeagent_telemetry::ExplanationOutcome::Pending,
        Err(_) => edgeagent_telemetry::ExplanationOutcome::Failed,
    };
    edgeagent_telemetry::record_explanation_operation(
        edgeagent_telemetry::ExplanationStage::Completion,
        outcome,
        started.elapsed(),
    );
    result
}
async fn execute_inner(
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
    match &recovery {
        Recovery::Terminal => return Ok(store.recover_explanation(&job.request, &job.job).await?),
        Recovery::Pending { .. } => return Ok(None),
        _ => {}
    }
    // Recovery has no runtime, owner or manifest dependency.
    let (manifest, manifest_id) = Manifest::environment()?;
    let profile = manifest.profile()?;
    let input = store.explanation_input(&job).await?;
    let decision =
        publication::decode_decision(job.original.receipt.decision().ok_or("missing decision")?)?;
    let selection = job.original.selection.ok_or("missing selection")?;
    let loader = OwnerLoader::from_explanation_input(&selection.profile(scope), &input)?;
    let evidence =
        tokio::time::timeout(Duration::from_secs(25), loader.load_execution(&input)).await??;
    let prepared = RuntimePreparation::from_verified(
        job.original.receipt.record(),
        &decision,
        selection.bytes(),
        &evidence,
        None,
    )?;
    let binding = prepared.binding(&profile)?;
    let serving = Request {
        anchor: binding.anchor.clone(),
        audience: std::env::var("EDGEAGENT_RESEARCH_EXPLANATION_AUDIENCE")?,
        purpose: Purpose::ExplanationServing,
        document: None,
    };
    EvidenceId::new(&serving.audience)?;
    let owner = OwnerEndpoint::from_environment()?;
    let mut grant = match authorize(&owner, &input, &serving).await {
        Ok(grant) => grant,
        Err(reason) => {
            return commit(
                store,
                &job.request,
                &binding,
                Completion::Refused(reason),
                None,
            )
            .await;
        }
    };
    if store
        .explanation_grant(&serving, clock_ms()?)
        .await?
        .is_none()
    {
        return commit(
            store,
            &job.request,
            &binding,
            Completion::Refused(Refusal::DisclosureDenied),
            None,
        )
        .await;
    }
    let standard = prepared.standard(None)?;
    let fallback = |reason| Completion::Fallback {
        explanation: standard.validated(),
        reason,
    };
    if let Recovery::Fallback(spent) = &recovery {
        if spent.intent().binding.anchor != binding.anchor
            || spent.intent().binding.profile != binding.profile
            || spent.intent().binding.evidence != binding.evidence
            || spent.intent().input_digest != prepared.input_digest()
        {
            return Err("original input required".into());
        }
        return commit(
            store,
            &job.request,
            &binding,
            fallback(Fallback::UncertainAttempt),
            Some(&grant),
        )
        .await;
    }
    let Some(approval) = store.qualification_approval(scope, &profile).await? else {
        return commit(
            store,
            &job.request,
            &binding,
            fallback(Fallback::Disabled),
            Some(&grant),
        )
        .await;
    };
    let (contract, _, _) =
        edgeagent_research_application::explanation_qualification::runtime_protocol()?;
    if approval.manifest != manifest_id
        || approval.helper != manifest.helper.sha256
        || approval.preparation != contract
    {
        return commit(
            store,
            &job.request,
            &binding,
            fallback(Fallback::Disabled),
            Some(&grant),
        )
        .await;
    }
    let audience = std::env::var("EDGEAGENT_RESEARCH_MODEL_AUDIENCE")?;
    let model_request = Request {
        anchor: binding.anchor.clone(),
        audience: audience.clone(),
        purpose: Purpose::ModelDisclosure,
        document: None,
    };
    let Some(model_grant) = store.explanation_grant(&model_request, clock_ms()?).await? else {
        return commit(
            store,
            &job.request,
            &binding,
            Completion::Refused(Refusal::DisclosureDenied),
            None,
        )
        .await;
    };
    let ResearchEvidence::Ready {
        decision: acquisition,
        ..
    } = ResearchEvidence::from_envelope(&input)?
    else {
        return Err("owner evidence required".into());
    };
    let EvidenceOutcome::Ready { object } = acquisition.outcome else {
        return Err("owner object required".into());
    };
    let operation = challenge()?;
    let mut disclosure = disclosure::Request {
        operation,
        research: job.request.clone(),
        anchor: binding.anchor.clone(),
        acquisition: acquisition.request.request,
        publication: digest(&input.to_json()?),
        evidence: object.sha256,
        audience: audience.clone(),
        purpose: disclosure::Purpose::ModelDisclosure,
        profile: profile.clone(),
        input_contract: contract,
        input: prepared.input_digest(),
    };
    // Authenticate an owner response before asking the supervisor to start anything.
    if let Err(reason) = owner_permit(&owner, &disclosure).await {
        return commit(
            store,
            &job.request,
            &binding,
            Completion::Refused(reason),
            None,
        )
        .await;
    }
    let prompt = manifest.verify(&profile)?;
    let observation = manifest.observation()?;
    let port = observation.port;
    let verifier = Verifier::new(observation, profile.deployment.clone())?;
    let model = HttpModel::new(
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, port),
        verifier.clone(),
        prompt,
    )?;
    // Startup consumed time; repeat owner authority immediately before reservation.
    disclosure.operation = challenge()?;
    let permit = match owner_permit(&owner, &disclosure).await {
        Ok(permit) => permit,
        Err(reason) => {
            return commit(
                store,
                &job.request,
                &binding,
                Completion::Refused(reason),
                None,
            )
            .await;
        }
    };
    let now = clock_ms()?;
    let edgeagent_contracts::research::ResearchRecord::Artifact(artifact) =
        job.original.receipt.record()
    else {
        return Err("artifact required".into());
    };
    if !(job.request.decision_at_ms..artifact.expires_at_ms).contains(&now) {
        return commit(
            store,
            &job.request,
            &binding,
            Completion::Refused(Refusal::DisclosureDenied),
            None,
        )
        .await;
    }
    let deadline = (now + i64::from(profile.settings.timeout_ms))
        .min(permit.expires_at_ms)
        .min(model_grant.expires_at_ms)
        .min(artifact.expires_at_ms);
    let intent = Intent {
        binding: binding.clone(),
        input_digest: prepared.input_digest(),
        audience,
        deadline_ms: deadline,
    };
    let authority = QualifiedAuthority {
        owner: permit,
        manifest: manifest_id,
        helper: manifest.helper.sha256.clone(),
        revision: approval.revision,
        assessment: approval.assessment,
    };
    let reservation = store
        .reserve_qualified_explanation_attempt(&intent, authority, || clock_ms().unwrap_or(-1))
        .await?;
    let ReservationOutcome::Reserved(mut reservation) = reservation else {
        return Ok(None);
    };
    let current = store.qualification_approval(scope, &profile).await?;
    if !current.is_some_and(|a| a.revision == approval.revision)
        || store
            .explanation_grant(&model_request, clock_ms()?)
            .await?
            .is_none()
    {
        return commit(
            store,
            &job.request,
            &binding,
            Completion::Refused(Refusal::DisclosureDenied),
            None,
        )
        .await;
    }
    let request = edgeagent_research_application::explanation_model::Request::new(
        &profile,
        &binding,
        prepared.input(),
    )?;
    let remaining = deadline - clock_ms()?;
    if remaining <= 0 {
        return commit(
            store,
            &job.request,
            &binding,
            fallback(Fallback::Timeout),
            Some(&grant),
        )
        .await;
    }
    let reply = tokio::time::timeout(
        Duration::from_millis(u64::try_from(remaining)?),
        model.propose(&request),
    )
    .await;
    verifier.verify()?;
    manifest.verify(&profile)?;
    // Fresh owner checks are external to the retaining transaction.
    disclosure.operation = challenge()?;
    let fresh = match owner_permit(&owner, &disclosure).await {
        Ok(permit) => permit,
        Err(reason) => {
            return commit(
                store,
                &job.request,
                &binding,
                Completion::Refused(reason),
                None,
            )
            .await;
        }
    };
    reservation.refresh_owner(fresh, clock_ms()?)?;
    grant = match authorize(&owner, &input, &serving).await {
        Ok(grant) => grant,
        Err(reason) => {
            return commit(
                store,
                &job.request,
                &binding,
                Completion::Refused(reason),
                None,
            )
            .await;
        }
    };
    match reply {
        Ok(Ok(reply)) => match prepared.validate(reply.proposal(), None) {
            Ok(presentation) => {
                commit(
                    store,
                    &job.request,
                    &binding,
                    Completion::Generated {
                        explanation: presentation.validated(),
                        permit: &reservation,
                    },
                    Some(&grant),
                )
                .await
            }
            Err(_) => {
                commit(
                    store,
                    &job.request,
                    &binding,
                    fallback(Fallback::InvalidProposal),
                    Some(&grant),
                )
                .await
            }
        },
        error => {
            let reason = match error {
                Err(_) | Ok(Err(Failure::Timeout)) => Fallback::Timeout,
                Ok(Err(Failure::BudgetExhausted)) => Fallback::BudgetExhausted,
                Ok(Err(Failure::InvalidResponse)) => Fallback::InvalidProposal,
                _ => Fallback::Unavailable,
            };
            commit(
                store,
                &job.request,
                &binding,
                fallback(reason),
                Some(&grant),
            )
            .await
        }
    }
}
async fn owner_permit(
    owner: &OwnerEndpoint,
    request: &disclosure::Request,
) -> std::result::Result<disclosure::Permit, Refusal> {
    match tokio::time::timeout(Duration::from_secs(5), read_owner_permit(owner, request)).await {
        Ok(Ok(Some(permit))) => Ok(permit),
        Ok(Ok(None)) => Err(Refusal::DisclosureDenied),
        _ => Err(Refusal::AuthorityUnavailable),
    }
}
async fn read_owner_permit(
    owner: &OwnerEndpoint,
    request: &disclosure::Request,
) -> Result<Option<disclosure::Permit>> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    async {
        let mut child = owner
            .command(&request.research.scope)
            .args([
                "offline-model-disclosure-permit",
                request.acquisition.as_str(),
            ])
            .spawn()?;
        let mut stdin = child.stdin.take().ok_or("owner stdin")?;
        let stdout = child.stdout.take().ok_or("owner stdout")?;
        let bytes = request.canonical_bytes()?;
        let writing = async {
            stdin.write_all(&bytes).await?;
            drop(stdin);
            Ok::<_, std::io::Error>(())
        };
        let reading = async {
            let mut bytes = Vec::new();
            stdout.take(16385).read_to_end(&mut bytes).await?;
            Ok::<_, std::io::Error>(bytes)
        };
        let (written, read) = tokio::join!(writing, reading);
        let status = child.wait().await?;
        if status.code() == Some(77) {
            return Ok(None);
        }
        written?;
        if !status.success() {
            return Err("owner model disclosure refused".into());
        }
        let permit = disclosure::Permit::decode(&read?)?;
        permit.bind(request, clock_ms()?)?;
        Ok(Some(permit))
    }
    .await
}

fn challenge() -> Result<EvidenceId> {
    let mut nonce = [0; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    Ok(EvidenceId::new(
        &nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )?)
}
