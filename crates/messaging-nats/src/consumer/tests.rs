use super::*;
use async_nats::jetstream::consumer::Config as ConsumerConfig;

#[test]
fn header_only_consumer_is_rejected_before_intake() {
    let config = ConsumerConfig {
        durable_name: Some("headers_only_v1".to_owned()),
        ack_policy: AckPolicy::Explicit,
        headers_only: true,
        ..ConsumerConfig::default()
    };
    assert_eq!(
        validate_consumer_configuration(&config)
            .err()
            .map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );
}

#[test]
fn consumer_requires_durable_explicit_acknowledgement() {
    assert_eq!(
        validate_consumer_configuration(&ConsumerConfig::default())
            .err()
            .map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );
    let durable_without_ack = ConsumerConfig {
        durable_name: Some("execution_simulator_v1".to_owned()),
        ack_policy: AckPolicy::None,
        ..ConsumerConfig::default()
    };
    assert_eq!(
        validate_consumer_configuration(&durable_without_ack)
            .err()
            .map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );
    let valid = ConsumerConfig {
        durable_name: Some("execution_simulator_v1".to_owned()),
        ack_policy: AckPolicy::Explicit,
        ..ConsumerConfig::default()
    };
    assert!(validate_consumer_configuration(&valid).is_ok());
}

#[test]
fn consumer_debug_exposes_only_lifecycle_state() {
    for intake in [ConsumerIntake::Halted, ConsumerIntake::Closed] {
        let consumer = JetStreamConsumer { intake };
        let debug = format!("{consumer:?}");
        assert!(debug.contains("intake"));
        assert!(!debug.contains("configured"));
        assert!(!debug.contains("initial_stream"));
    }
}
