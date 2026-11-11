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
