use super::*;
use edgeagent_messaging::RetryDelay;

fn confirmation(status: Option<StatusCode>, payload: &[u8]) -> async_nats::Message {
    async_nats::Message {
        subject: "confirmation.inbox".into(),
        reply: None,
        payload: payload.to_vec().into(),
        headers: None,
        status,
        description: Some("private-confirmation-description-sentinel".to_owned()),
        length: payload.len(),
    }
}

#[test]
fn only_successful_empty_responses_confirm_settlement() -> Result<(), Box<dyn Error>> {
    for status in [None, Some(StatusCode::OK)] {
        assert!(validate_settlement_confirmation(&confirmation(status, b"")).is_ok());
    }
    for status in [
        StatusCode::NO_RESPONDERS,
        StatusCode::REQUEST_TERMINATED,
        StatusCode::IDLE_HEARTBEAT,
    ] {
        let result = validate_settlement_confirmation(&confirmation(Some(status), b""));
        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(ConsumeErrorKind::ConfirmationUnknown)
        );
    }
    for status in [None, Some(StatusCode::OK)] {
        let result = validate_settlement_confirmation(&confirmation(
            status,
            b"private-confirmation-body-sentinel",
        ));
        let error = result.err().ok_or("nonempty confirmation must fail")?;
        assert_eq!(error.kind(), ConsumeErrorKind::ConfirmationUnknown);
        assert!(!format!("{error} {error:?}").contains("private-confirmation"));
        let source = error.source().map(ToString::to_string).unwrap_or_default();
        assert!(!source.contains("private-confirmation"));
        assert!(source.contains("payload bytes"));
    }
    Ok(())
}

#[test]
fn settlement_maps_to_confirmed_acknowledgement_kinds() -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        acknowledgement_kind(DeliveryDisposition::Acknowledge),
        AckKind::Ack
    ));
    assert!(matches!(
        acknowledgement_kind(DeliveryDisposition::Quarantined),
        AckKind::Term
    ));
    let delay = RetryDelay::new(Duration::from_secs(2))?;
    assert!(matches!(
        acknowledgement_kind(DeliveryDisposition::RetryAfter(delay)),
        AckKind::Nak(Some(delay)) if delay == Duration::from_secs(2)
    ));
    Ok(())
}
