use super::*;

#[test]
fn delivery_message_keys_are_stable_and_unambiguous() {
    assert_eq!(
        delivery_message_key("ORDERS", 41).ok().as_deref(),
        Some("6:ORDERS:41")
    );
    assert_eq!(
        delivery_message_key("", 41).err().map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );
    assert_eq!(
        delivery_message_key("ORDERS", 0)
            .err()
            .map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );
}

#[test]
fn broker_consumer_sequence_is_validated_without_leaking_into_portable_metadata() {
    assert_eq!(
        validate_consumer_sequence(0)
            .err()
            .map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );
    assert!(validate_consumer_sequence(1).is_ok());
}
