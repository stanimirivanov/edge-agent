//! Pure replay orchestration with real frozen-byte conversion and no publication port.
#[allow(dead_code)]
mod support;
use edgeagent_contracts::MessageEnvelope;
use edgeagent_research_application::{
    RENDERER_REVISION, evaluate,
    publication::{self, CommittedOutcome, PreparedCompletion},
    replay::{self, ReplayError},
    worker::{EvidenceLoader, LoadError, LoadedEvidence},
};
use std::sync::atomic::{AtomicUsize, Ordering};
use support::*;
const MOMENTUM: &str = edgeagent_research_domain::momentum::REVISION;
struct Loader {
    bytes: Vec<u8>,
    reverse: bool,
    error: Option<LoadError>,
    calls: AtomicUsize,
}
impl EvidenceLoader for Loader {
    async fn load(
        &self,
        delivery: &MessageEnvelope,
    ) -> std::result::Result<LoadedEvidence, LoadError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut loaded = edgeagent_research::loader::convert(
            delivery,
            &self.bytes,
            [2; 32],
            [3; 32],
            1_209_600_000,
            60_000,
        )?;
        if self.reverse {
            loaded.series.reverse();
            for series in &mut loaded.series {
                series.closes.reverse();
            }
        }
        Ok(loaded)
    }
}
fn loader(bytes: Vec<u8>) -> Loader {
    Loader {
        bytes,
        reverse: false,
        error: None,
        calls: AtomicUsize::new(0),
    }
}
async fn receipt(
    loader: &Loader,
    event: &MessageEnvelope,
    expiry_delta: i64,
) -> Result<CommittedOutcome> {
    let loaded = loader.load(event).await?;
    let result = evaluate(
        publication::evaluation_request(event)?,
        &loaded.policy,
        &loaded.series,
        loaded.expires_at_ms + expiry_delta,
    )?;
    let prepared = PreparedCompletion::new(event.clone(), &result, "2026-10-30T00:00:00Z")?;
    Ok(CommittedOutcome::decode(
        result.bytes(),
        &prepared.publication().to_json()?,
    )?)
}
#[tokio::test]
async fn replay_recalculates_exact_bytes_under_input_permutation() -> Result {
    let (event, bytes) = event("replay")?;
    let original = loader(bytes.clone());
    let receipt = receipt(&original, &event, 0).await?;
    for reverse in [false, true] {
        let loader = Loader {
            reverse,
            ..loader(bytes.clone())
        };
        for _ in 0..2 {
            let result =
                replay::verify(&loader, &event, &receipt, RENDERER_REVISION, MOMENTUM).await?;
            assert_eq!(result.bytes(), receipt.bytes());
        }
        assert_eq!(loader.calls.load(Ordering::SeqCst), 2);
    }
    Ok(())
}
#[tokio::test]
async fn replay_refuses_changed_revisions_intent_and_output() -> Result {
    let (event, bytes) = event("replay")?;
    let loader = loader(bytes);
    let good = receipt(&loader, &event, 0).await?;
    loader.calls.store(0, Ordering::SeqCst);
    for (renderer, momentum) in [
        ("new-renderer", MOMENTUM),
        (RENDERER_REVISION, "new-momentum"),
    ] {
        assert!(matches!(
            replay::verify(&loader, &event, &good, renderer, momentum).await,
            Err(ReplayError::Revision)
        ));
    }
    let (foreign, _) = support::event("other-intent")?;
    assert!(matches!(
        replay::verify(&loader, &foreign, &good, RENDERER_REVISION, MOMENTUM).await,
        Err(ReplayError::Binding)
    ));
    assert_eq!(loader.calls.load(Ordering::SeqCst), 0);
    let changed = receipt(&loader, &event, 1).await?;
    assert!(matches!(
        replay::verify(&loader, &event, &changed, RENDERER_REVISION, MOMENTUM).await,
        Err(ReplayError::Mismatch)
    ));
    Ok(())
}
#[tokio::test]
async fn replay_read_failures_never_become_successful_abstentions() -> Result {
    let (event, bytes) = event("replay")?;
    let good = receipt(&loader(bytes.clone()), &event, 0).await?;
    for error in [
        LoadError::Denied,
        LoadError::Invalid,
        LoadError::Unsupported,
        LoadError::Unavailable,
    ] {
        let failed = Loader {
            error: Some(error),
            ..loader(bytes.clone())
        };
        let failure = replay::verify(&failed, &event, &good, RENDERER_REVISION, MOMENTUM)
            .await
            .err()
            .ok_or("expected refusal")?;
        assert!(matches!(failure,ReplayError::Read(value) if value==error));
        assert!(std::error::Error::source(&failure).is_some());
        assert!(!format!("{failure:?} {failure}").contains("private-test-sentinel"));
    }
    let mut corrupt = bytes;
    corrupt.push(b' ');
    assert!(matches!(
        replay::verify(&loader(corrupt), &event, &good, RENDERER_REVISION, MOMENTUM).await,
        Err(ReplayError::Read(LoadError::Invalid))
    ));
    Ok(())
}

#[tokio::test]
async fn replay_owner_declared_abstention_is_verified_without_evidence_reads() -> Result {
    use edgeagent_contracts::{research::AbstentionReason, research_acquisition::ResearchEvidence};
    let (event, bytes) = event("declined")?;
    let request = publication::evaluation_request(&event)?;
    let reason = AbstentionReason::InsufficientHistory;
    let mut value: serde_json::Value = serde_json::from_slice(&event.to_json()?)?;
    value["data"] = serde_json::to_value(ResearchEvidence::Abstained {
        request: request.clone(),
        reason,
    })?;
    let declared = MessageEnvelope::from_json(&serde_json::to_vec(&value)?)?;
    let result = edgeagent_research_application::abstain(request, reason)?;
    let prepared = PreparedCompletion::new(declared.clone(), &result, "2026-10-30T00:00:00Z")?;
    let retained = CommittedOutcome::decode(result.bytes(), &prepared.publication().to_json()?)?;
    let loader = Loader {
        error: Some(LoadError::Unavailable),
        ..loader(bytes)
    };
    assert_eq!(
        replay::verify(&loader, &declared, &retained, RENDERER_REVISION, MOMENTUM)
            .await?
            .bytes(),
        result.bytes()
    );
    assert_eq!(loader.calls.load(Ordering::SeqCst), 0);
    // A Ready delivery's historical read-failure abstention is different: a current
    // denial cannot certify reproduction of the original historical failure.
    let original_request = publication::evaluation_request(&event)?;
    let abstained = edgeagent_research_application::abstain(
        original_request,
        AbstentionReason::MissingEntitlement,
    )?;
    let prepared = PreparedCompletion::new(event.clone(), &abstained, "2026-10-30T00:00:00Z")?;
    let retained = CommittedOutcome::decode(abstained.bytes(), &prepared.publication().to_json()?)?;
    assert!(matches!(
        replay::verify(&loader, &event, &retained, RENDERER_REVISION, MOMENTUM).await,
        Err(ReplayError::Read(LoadError::Unavailable))
    ));
    Ok(())
}
