//! Wire correlation, owner source and strict refusal proofs.
use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    research::{AbstentionReason, ResearchRecord, ResearchRequest},
    research_acquisition::{self, DECIDED, REQUEST, ResearchEvidence},
};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn request() -> ResearchRequest {
    ResearchRequest {
        scope: "operator".into(),
        request_id: "research-1".into(),
        implementation: "02".repeat(32),
        configuration: "03".repeat(32),
        decision_at_ms: 100,
        lookback_sessions: 5,
        instruments: vec![format!("{:032x}", 1)],
    }
}
fn metadata(event: bool) -> Result<MessageMetadata> {
    let (key, partition) = research_acquisition::binding(&request())?;
    let def = if event { DECIDED } else { REQUEST };
    Ok(MessageMetadata {
        id: "delivery-1".into(),
        source: if event {
            Component::MarketData
        } else {
            Component::Gateway
        }
        .source_uri()
        .into(),
        message_type: def.message_type().into(),
        subject: partition.clone(),
        time: "2026-10-29T00:00:00Z".into(),
        data_schema: def.data_schema().into(),
        correlation_id: key.clone(),
        causation_id: "cause-1".into(),
        idempotency_key: key,
        partition_key: partition,
        trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
        trace_state: None,
    })
}
#[test]
fn request_and_outcome_reject_wrong_owner_route_scope_and_correlation() -> Result {
    let record = ResearchRecord::Request(request());
    assert!(
        research_acquisition::request(&MessageEnvelope::from_payload(metadata(false)?, &record)?)?
            == request()
    );
    let outcome = ResearchEvidence::Abstained {
        request: request(),
        reason: AbstentionReason::MissingEntitlement,
    };
    assert!(
        ResearchEvidence::from_envelope(&MessageEnvelope::from_payload(
            metadata(true)?,
            &outcome
        )?)? == outcome
    );
    for event in [false, true] {
        for field in 0..5 {
            let mut m = metadata(event)?;
            match field {
                0 => m.source = Component::Research.source_uri().into(),
                1 => m.partition_key = "research-request/foreign/id".into(),
                2 => m.correlation_id = "foreign".into(),
                3 => m.idempotency_key = "foreign".into(),
                _ => m.subject = "research-request/foreign/id".into(),
            }
            let rejected = if event {
                ResearchEvidence::from_envelope(&MessageEnvelope::from_payload(m, &outcome)?)
                    .is_err()
            } else {
                research_acquisition::request(&MessageEnvelope::from_payload(m, &record)?).is_err()
            };
            assert!(rejected);
        }
    }
    let mut value = serde_json::to_value(outcome)?;
    value["object"] = true.into();
    assert!(serde_json::from_value::<ResearchEvidence>(value).is_err());
    Ok(())
}
#[test]
#[cfg(feature = "schema")]
fn correlated_evidence_schema_matches_frozen_inventory() -> Result {
    assert_eq!(
        research_acquisition::schema()?,
        include_str!("../fixtures/research/evidence-v1-schema.json")
    );
    Ok(())
}
