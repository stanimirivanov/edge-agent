use std::{error::Error, time::Duration};

use async_nats::jetstream::{
    consumer::IntoConsumerConfig,
    stream::{DiscardPolicy, RetentionPolicy, StorageType},
};
use edgeagent_contracts::{Component, EventRetention, MessageDefinition, MessageRegistry};

use super::*;

const COMMAND: MessageDefinition = MessageDefinition::command(
    "com.edgeagent.conformance.apply-probe.v1",
    "urn:edgeagent:schema:conformance:apply-probe:v1",
    Component::Research,
    "probe",
);
const EVENT: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.conformance.probe-applied.v1",
    "urn:edgeagent:schema:conformance:probe-applied:v1",
    Component::Research,
    "probe",
    EventRetention::Workflow,
);
const AUDIT: MessageDefinition = MessageDefinition::event(
    "com.edgeagent.conformance.probe-audited.v1",
    "urn:edgeagent:schema:conformance:probe-audited:v1",
    Component::AuditProjector,
    "probe",
    EventRetention::Audit,
);
type TestResult = Result<(), Box<dyn Error>>;

fn plan(definitions: &[MessageDefinition]) -> Result<TopologyPlan, Box<dyn Error>> {
    Ok(TopologyPlan::new(
        MessageRegistry::new(definitions)?,
        "m02",
        &[Binding::new(Component::Gateway, COMMAND.message_type())],
        &[Binding::new(
            Component::AuditProjector,
            EVENT.message_type(),
        )],
        TopologyLimits::default(),
    )?)
}

#[test]
fn registry_plan_is_deterministic_and_preserves_retention_and_ownership() -> TestResult {
    let expected = plan(&[COMMAND, EVENT, AUDIT])?;
    assert_eq!(expected, plan(&[AUDIT, EVENT, COMMAND])?);
    for stream in expected.streams() {
        let config = stream.config();
        assert_eq!(config.subjects, [stream.definition().subject()?]);
        assert_eq!(config.storage, StorageType::File);
        assert_eq!(
            config.max_age.as_secs(),
            stream.definition().retention().seconds()
        );
        assert_eq!(config.discard, DiscardPolicy::New);
        assert_eq!(config.max_message_size, 263_168);
        assert_eq!(config.max_bytes, 64 * 1024 * 1024);
        assert!(config.deny_delete && config.deny_purge);
        assert!(!config.no_ack && !config.allow_message_ttl && !config.allow_rollup);
        if stream.definition() == COMMAND {
            assert_eq!(config.retention, RetentionPolicy::WorkQueue);
            assert_eq!(stream.consumers().len(), 1);
            assert_eq!(stream.consumers()[0].component(), Component::Research);
        } else {
            assert_eq!(config.retention, RetentionPolicy::Limits);
        }
        for durable in stream.consumers() {
            let config = durable.config();
            assert_eq!(config.max_deliver, -1);
            assert_eq!(config.max_batch, 1);
            assert_eq!(config.max_ack_pending, 32);
            assert_eq!(config.max_waiting, 16);
            assert_eq!(config.ack_wait, Duration::from_secs(120));
            assert!(config.durable_name.is_some());
            assert!(!config.memory_storage && !config.headers_only);
        }
    }
    Ok(())
}

#[test]
fn invalid_names_registries_and_bindings_fail_before_io() -> TestResult {
    let definitions = [COMMAND, EVENT];
    let registry = MessageRegistry::new(&definitions)?;
    for namespace in [
        "",
        "INVALID",
        "x.y",
        "x_",
        ">",
        "space here",
        &"a".repeat(33),
    ] {
        assert_eq!(
            TopologyPlan::new(registry, namespace, &[], &[], TopologyLimits::default())
                .err()
                .ok_or("accepted invalid namespace")?
                .kind(),
            TopologyErrorKind::InvalidInput
        );
    }
    for (publishers, subscribers) in [
        (
            vec![Binding::new(Component::Gateway, "unregistered")],
            vec![],
        ),
        (
            vec![Binding::new(Component::Gateway, EVENT.message_type())],
            vec![],
        ),
        (
            vec![],
            vec![Binding::new(Component::Gateway, COMMAND.message_type())],
        ),
        (
            vec![Binding::new(Component::Gateway, COMMAND.message_type()); 2],
            vec![],
        ),
        (
            vec![],
            vec![Binding::new(Component::Gateway, EVENT.message_type()); 2],
        ),
    ] {
        assert!(
            TopologyPlan::new(
                registry,
                "m02",
                &publishers,
                &subscribers,
                TopologyLimits::default()
            )
            .is_err()
        );
    }
    assert!(
        TopologyPlan::new(
            MessageRegistry::new(&[])?,
            "m02",
            &[],
            &[],
            TopologyLimits::default()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn admission_limits_accept_inclusive_bounds_and_reject_unbounded_values() {
    assert!(TopologyLimits::new(1, 263_168, 1, 1).is_ok());
    assert!(TopologyLimits::new(1_000_000, 1_099_511_627_776, 1024, 1024).is_ok());
    for (messages, bytes, pending, waiting) in [
        (0, 263_168, 1, 1),
        (-1, 263_168, 1, 1),
        (1_000_001, 263_168, 1, 1),
        (1, 263_167, 1, 1),
        (1, 1_099_511_627_777, 1, 1),
        (1, 263_168, 0, 1),
        (1, 263_168, 1025, 1),
        (1, 263_168, 1, 0),
        (1, 263_168, 1, 1025),
    ] {
        assert!(TopologyLimits::new(messages, bytes, pending, waiting).is_err());
    }
}

#[test]
fn runtime_grants_exclude_topology_mutation_foreign_inboxes_and_unbound_subjects() -> TestResult {
    let topology = plan(&[COMMAND, EVENT])?;
    for role in topology.runtime_permissions() {
        assert_eq!(
            role.subscribe_allow().iter().cloned().collect::<Vec<_>>(),
            [format!("{}.>", role.inbox_prefix())]
        );
        assert!(!role.nats_config().contains("allow_responses"));
        assert!(
            role.publish_allow()
                .iter()
                .all(|subject| !subject.contains("CREATE")
                    && !subject.contains("DELETE")
                    && !subject.contains("UPDATE")
                    && !subject.contains("PURGE")
                    && !subject.ends_with("INFO.>"))
        );
        if role.component() == Some(Component::Gateway) {
            assert_eq!(
                role.publish_allow().iter().cloned().collect::<Vec<_>>(),
                [COMMAND.subject()?]
            );
        }
        if role.component() == Some(Component::AuditProjector) {
            assert!(!role.publish_allow().contains(&EVENT.subject()?));
        }
    }
    let provisioner = topology.provisioner_permissions();
    assert!(provisioner.component().is_none());
    assert!(
        provisioner
            .publish_allow()
            .iter()
            .all(|subject| subject.starts_with("$JS.API."))
    );
    assert!(
        provisioner
            .publish_allow()
            .iter()
            .all(|subject| !subject.contains("DELETE")
                && !subject.contains("UPDATE")
                && !subject.contains("PURGE")
                && !subject.contains("MSG.NEXT"))
    );
    Ok(())
}

#[test]
fn full_stream_configuration_drift_fails_closed() -> TestResult {
    let topology = plan(&[COMMAND, EVENT])?;
    let expected = topology.streams()[0].config();
    provision::verify_stream(expected, expected)?;
    let mut variants = Vec::new();
    let mut asynchronous = expected.clone();
    asynchronous.persist_mode = Some(async_nats::jetstream::stream::PersistenceMode::Async);
    variants.push(asynchronous);
    let mut storage = expected.clone();
    storage.storage = StorageType::Memory;
    variants.push(storage);
    let mut eviction = expected.clone();
    eviction.discard = DiscardPolicy::Old;
    variants.push(eviction);
    let mut unlimited = expected.clone();
    unlimited.max_bytes = -1;
    variants.push(unlimited);
    let mut expiry = expected.clone();
    expiry.max_age = Duration::from_secs(1);
    variants.push(expiry);
    let mut wildcard = expected.clone();
    wildcard.subjects = vec!["edgeagent.>".into()];
    variants.push(wildcard);
    let mut ttl = expected.clone();
    ttl.allow_message_ttl = true;
    variants.push(ttl);
    let mut rollup = expected.clone();
    rollup.allow_rollup = true;
    variants.push(rollup);
    let mut ack = expected.clone();
    ack.no_ack = true;
    variants.push(ack);
    let mut metadata = expected.clone();
    metadata
        .metadata
        .insert("unreviewed".into(), "changed".into());
    variants.push(metadata);
    for actual in variants {
        assert_eq!(
            provision::verify_stream(&actual, expected)
                .err()
                .ok_or("accepted drift")?
                .kind(),
            TopologyErrorKind::Conflict
        );
    }
    let mut informational = expected.clone();
    informational
        .metadata
        .insert("_nats.ver".into(), "2.14.7".into());
    informational.persist_mode = None;
    provision::verify_stream(&informational, expected)?;
    Ok(())
}

#[test]
fn full_consumer_configuration_drift_fails_closed() -> TestResult {
    let topology = plan(&[COMMAND, EVENT])?;
    let expected = topology
        .streams()
        .iter()
        .find(|stream| stream.definition() == COMMAND)
        .ok_or("command stream missing")?
        .consumers()[0]
        .config()
        .clone()
        .into_consumer_config();
    provision::verify_consumer(&expected, &expected)?;
    let mut variants = Vec::new();
    let mut ephemeral = expected.clone();
    ephemeral.durable_name = None;
    variants.push(ephemeral);
    let mut delivery_limit = expected.clone();
    delivery_limit.max_deliver = 3;
    variants.push(delivery_limit);
    let mut pending = expected.clone();
    pending.max_ack_pending = -1;
    variants.push(pending);
    let mut waiting = expected.clone();
    waiting.max_waiting = -1;
    variants.push(waiting);
    let mut push = expected.clone();
    push.deliver_subject = Some("private-sentinel".into());
    variants.push(push);
    let mut headers = expected.clone();
    headers.headers_only = true;
    variants.push(headers);
    let mut ack = expected.clone();
    ack.ack_policy = async_nats::jetstream::consumer::AckPolicy::None;
    variants.push(ack);
    let mut filter = expected.clone();
    filter.filter_subjects = vec!["edgeagent.>".into()];
    variants.push(filter);
    for actual in variants {
        assert_eq!(
            provision::verify_consumer(&actual, &expected)
                .err()
                .ok_or("accepted consumer drift")?
                .kind(),
            TopologyErrorKind::Conflict
        );
    }
    Ok(())
}

#[test]
fn topology_errors_redact_causes_and_preserve_auto_traits() {
    let error = TopologyError::io(std::io::Error::other("private-credential-sentinel"));
    assert!(error.source().is_some());
    assert!(!format!("{error} {error:?}").contains("private-credential-sentinel"));
    assert!(
        TopologyError::new(TopologyErrorKind::Conflict)
            .source()
            .is_none()
    );
    fn send_sync<T: Send + Sync + 'static>() {}
    send_sync::<TopologyPlan>();
    send_sync::<TopologyError>();
}
