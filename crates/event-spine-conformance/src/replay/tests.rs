use super::*;
use crate::{definitions, registry};
use edgeagent_messaging_nats::topology::{Binding, TopologyLimits};

fn plan() -> Result<TopologyPlan, AcceptanceError> {
    TopologyPlan::new(
        registry()?,
        "replay-test",
        &[Binding::new(
            Component::Gateway,
            definitions()[0].message_type(),
        )],
        &[Binding::new(
            Component::AuditProjector,
            definitions()[1].message_type(),
        )],
        TopologyLimits::default(),
    )
    .map_err(|_| AcceptanceError::Routing)
}

#[test]
fn replay_scope_has_only_incoming_subjects_and_an_isolated_reply_inbox()
-> Result<(), AcceptanceError> {
    let plan = plan()?;
    let scope = ReplayPublisherScope::new(&plan, Component::Research)?;
    assert_eq!(
        scope.subjects(),
        &BTreeSet::from([definitions()[0]
            .subject()
            .map_err(|_| AcceptanceError::Routing)?])
    );
    assert!(!scope.nats_config().contains("$JS"));
    assert!(!scope.nats_config().contains("$JS.ACK"));
    let runtime = plan
        .runtime_permissions()
        .iter()
        .find(|p| p.component() == Some(Component::Research))
        .ok_or(AcceptanceError::Routing)?;
    assert!(
        !scope
            .inbox_prefix()
            .starts_with(&format!("{}.", runtime.inbox_prefix()))
    );
    assert!(ReplayPublisherScope::new(&plan, Component::Gateway).is_err());
    Ok(())
}

#[test]
fn replay_request_cannot_forge_actor_and_diagnostics_do_not_echo_input()
-> Result<(), serde_json::Error> {
    let request = r#"{"direction":"inbound","request_id":"private-sentinel","consumer_name":"owner","delivery_key":"key","reason":"remediated"}"#;
    let decoded: ReplayCommand = serde_json::from_str(request)?;
    assert!(!format!("{decoded:?}").contains("private-sentinel"));
    let forged = request.replace("\"reason\"", "\"requested_by\":\"admin\",\"reason\"");
    assert!(serde_json::from_str::<ReplayCommand>(&forged).is_err());
    Ok(())
}

#[test]
fn retained_bytes_require_supported_payload_exact_route_and_owning_subscriber()
-> Result<(), AcceptanceError> {
    let plan = plan()?;
    let bytes = include_bytes!("../../fixtures/v1/apply-probe.json");
    let subject = definitions()[0]
        .subject()
        .map_err(|_| AcceptanceError::Routing)?;
    let consumer = Component::Research.as_str().replace('.', "_");
    validate_inbound(&plan, Component::Research, &consumer, &subject, bytes)?;
    for (component, consumer, subject, bytes) in [
        (
            Component::AuditProjector,
            consumer.as_str(),
            subject.as_str(),
            bytes.as_slice(),
        ),
        (
            Component::Research,
            "different_subscriber",
            subject.as_str(),
            bytes.as_slice(),
        ),
        (
            Component::Research,
            consumer.as_str(),
            "untrusted.subject",
            bytes.as_slice(),
        ),
        (
            Component::Research,
            consumer.as_str(),
            subject.as_str(),
            b"not-json".as_slice(),
        ),
    ] {
        assert!(validate_inbound(&plan, component, consumer, subject, bytes).is_err());
    }
    Ok(())
}
