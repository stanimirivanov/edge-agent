//! Internal historical verification. Successful replay does not authorize serving.
use crate::{loader::OwnerLoader, runtime::Result};
use edgeagent_contracts::explanation_profile::{MAX_PROFILE_BYTES, Profile};
use edgeagent_research_application::explanation_replay::{Retained, verify};
use edgeagent_research_postgres::PostgresResearchStore;
use std::io::Read as _;

pub(crate) async fn execute(store: &PostgresResearchStore, scope: &str) -> Result<Vec<u8>> {
    let id = std::env::var("EDGEAGENT_RESEARCH_EXPLANATION_JOB")?;
    crate::loader::digest(&id)?;
    let scheduled = store
        .explanation_job(scope, Some(&id))
        .await?
        .ok_or("missing replay job")?;
    let receipt = scheduled
        .receipt
        .as_ref()
        .ok_or("missing explanation completion")?;
    let input = store.explanation_input(&scheduled).await?;
    let original = store
        .retained_for_replay(&input)
        .await?
        .ok_or("missing original completion")?;
    let selection = original.selection.ok_or("missing pinned execution")?;
    let loader = OwnerLoader::from_explanation_input(&selection.profile(scope), &input)?;
    let mut bytes = Vec::new();
    std::fs::File::open(std::env::var("EDGEAGENT_RESEARCH_EXPLANATION_PROFILE")?)?
        .take((MAX_PROFILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let profile = Profile::decode(&bytes)?;
    let renderer = std::env::var("EDGEAGENT_RESEARCH_REPLAY_RENDERER")?;
    let momentum = std::env::var("EDGEAGENT_RESEARCH_REPLAY_MOMENTUM")?;
    if profile.renderer
        == crate::explanation_owned::digest(
            edgeagent_research_application::explanation_runtime::RENDERER_REVISION.as_bytes(),
        )
    {
        use edgeagent_contracts::explanation::{Companion, Outcome};
        use edgeagent_research_application::{
            explanation_runtime::RuntimePreparation, worker::ExecutionLoader,
        };
        if renderer != edgeagent_research_application::RENDERER_REVISION
            || momentum != edgeagent_research_domain::momentum::REVISION
        {
            return Err("unsupported replay revision".into());
        }
        let evidence = loader.load_execution(&input).await?;
        let decision = edgeagent_research_application::publication::decode_decision(
            original.receipt.decision().ok_or("decision")?,
        )?;
        let prepared = RuntimePreparation::from_verified(
            original.receipt.record(),
            &decision,
            selection.bytes(),
            &evidence,
            None,
        )?;
        let binding = prepared.binding(&profile)?;
        let companion = Companion::decode(receipt.bytes())?;
        companion.bind(original.receipt.record(), &decision)?;
        if companion.binding.anchor != binding.anchor
            || companion.binding.profile != binding.profile
            || companion.binding.evidence != binding.evidence
        {
            return Err("replay binding changed".into());
        }
        match companion.outcome {
            Outcome::Generated { proposal } => {
                prepared.validate(&proposal.canonical_bytes()?, None)?;
            }
            Outcome::Fallback { proposal, .. } => {
                if proposal.canonical_bytes()?
                    != prepared
                        .standard(None)?
                        .validated()
                        .proposal()
                        .canonical_bytes()?
                {
                    return Err("fallback changed".into());
                }
            }
            Outcome::Refused { .. } => return Err("private refusal".into()),
        }
        return Ok(receipt.bytes().to_vec());
    }
    verify(
        &loader,
        &input,
        &original.receipt,
        selection,
        Retained {
            profile: &profile,
            companion: receipt.bytes(),
        },
        &renderer,
        &momentum,
    )
    .await?;
    Ok(receipt.bytes().to_vec())
}
