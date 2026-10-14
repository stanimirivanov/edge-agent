//! Pure validation of untrusted broker delivery metadata.

use async_nats::jetstream;
use edgeagent_messaging::{
    ConsumeError, ConsumeErrorKind, DeliveryAttempt, DeliveryMessageKey, DeliveryMetadata,
    DeliverySubject,
};

#[cfg(test)]
mod tests;

pub(super) fn derive(message: &jetstream::Message) -> Result<DeliveryMetadata, ConsumeError> {
    let info = message
        .info()
        .map_err(|error| ConsumeError::with_boxed_source(ConsumeErrorKind::Protocol, error))?;
    let attempt = DeliveryAttempt::new(
        u32::try_from(info.delivered)
            .map_err(|error| ConsumeError::with_source(ConsumeErrorKind::Protocol, error))?,
    )?;
    let key = DeliveryMessageKey::new(delivery_message_key(info.stream, info.stream_sequence)?)?;
    validate_consumer_sequence(info.consumer_sequence)?;
    Ok(DeliveryMetadata::new(
        key,
        DeliverySubject::new(message.subject.to_string())?,
        attempt,
    ))
}

fn delivery_message_key(stream: &str, stream_sequence: u64) -> Result<String, ConsumeError> {
    if stream.is_empty() || stream_sequence == 0 {
        return Err(ConsumeError::protocol(
            "stream identity must contain a name and positive sequence",
        ));
    }
    Ok(format!("{}:{stream}:{stream_sequence}", stream.len()))
}

fn validate_consumer_sequence(sequence: u64) -> Result<(), ConsumeError> {
    if sequence == 0 {
        Err(ConsumeError::protocol(
            "consumer sequence must be greater than zero",
        ))
    } else {
        Ok(())
    }
}
