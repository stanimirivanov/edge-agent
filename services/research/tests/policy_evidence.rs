#[allow(dead_code)]
mod support;
use edgeagent_contracts::{
    evidence::*, policy_evidence::*, research_acquisition::ResearchEvidence,
};
use support::*;
#[test]
fn policy_conversion_requires_exact_request_and_daily_lineage() -> Result {
    let (delivery, bytes) = event("policy-read")?;
    let ResearchEvidence::Ready { request, decision } = ResearchEvidence::from_envelope(&delivery)?
    else {
        return Err("ready")?;
    };
    let EvidenceOutcome::Ready { object } = &decision.outcome else {
        return Err("object")?;
    };
    let daily = DailyEvidenceArtifact::decode(&bytes, &object.sha256)?;
    let companion = PolicyEvidence::V1 {
        request: request.clone(),
        daily: Box::new(daily),
        events: EventCoverage::Unknown,
    };
    let volumes = edgeagent_research::loader::policy_volumes(&companion)?;
    assert_eq!(volumes.rows.len(), 2);
    let PolicyEvidence::V1 { daily, .. } = &companion;
    assert_eq!(
        volumes.evidence,
        edgeagent_research::loader::digest(daily.digest().as_str())?
    );
    for (row, bar) in volumes.rows.iter().zip(&daily.data().bars) {
        let expected = edgeagent_market_data_domain::ShareVolume::new(
            bar.volume.coefficient.parse()?,
            u32::from(bar.volume.scale),
        )?;
        assert!(row.shares == expected);
    }
    let opens = edgeagent_research::loader::policy_opens(&companion)?;
    assert_eq!(opens.evidence, volumes.evidence);
    assert_eq!(opens.instrument, volumes.instrument);
    for (row, bar) in opens.rows.iter().zip(&daily.data().bars) {
        let expected = edgeagent_market_data_domain::UsdPrice::new(
            bar.ohlc[0].coefficient.parse()?,
            u32::from(bar.ohlc[0].scale),
        )?;
        assert!(row.price == expected);
        assert_eq!(row.action, !bar.actions.is_empty());
    }
    let context = edgeagent_research::loader::policy_context(&companion)?;
    assert_eq!(context.evidence, volumes.evidence);
    assert_eq!(context.instrument, volumes.instrument);
    assert!(matches!(
        context.coverage,
        edgeagent_research_domain::context_policy::Coverage::Unknown
    ));
    let PolicyEvidence::V1 {
        request: bound,
        daily: object,
        ..
    } = &companion;
    let positive = PolicyEvidence::V1 {
        request: bound.clone(),
        daily: object.clone(),
        events: EventCoverage::SyntheticComplete {
            revision: DailyEvidenceArtifact::hash(b"invented"),
            known_at_ms: bound.decision_at_ms,
            interval: [0, object.data().delete_by.seconds * 1000],
            events: vec![bound.decision_at_ms],
        },
    };
    assert!(
        matches!(edgeagent_research::loader::policy_context(&positive)?.coverage,edgeagent_research_domain::context_policy::Coverage::Complete{events,..} if events==[bound.decision_at_ms])
    );
    let exported = companion.bytes()?;
    assert!(edgeagent_research::loader::convert_policy(&delivery, &exported).is_ok());
    let PolicyEvidence::V1 {
        mut request,
        daily,
        events,
    } = companion;
    request.scope = "foreign".into();
    let foreign = PolicyEvidence::V1 {
        request,
        daily,
        events,
    }
    .bytes()?;
    assert!(edgeagent_research::loader::convert_policy(&delivery, &foreign).is_err());
    assert!(edgeagent_research::loader::convert_policy(&delivery, b"PRIVATE_SENTINEL").is_err());
    Ok(())
}
#[test]
fn retained_history_recipe_bytes_are_the_pre_policy_revision() {
    assert_eq!(
        DailyEvidenceArtifact::hash(include_bytes!(
            "../../market-data/fixtures/offline-history-v1/fixture.rs"
        ))
        .as_str(),
        "e7ecd90dfc53559f93795872453072e8cb015c2f12cf52974f1832b3b3960a4c"
    );
}
