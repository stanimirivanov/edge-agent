use edgeagent_contracts::{MessageContractError, MessageEnvelope, MessageRegistry};
use edgeagent_messaging::{
    InboundMessageStore, InboundProcessingError, InboxDisposition, MessageDelivery,
};

use crate::observability::SpineRecorder;
use crate::resolution::{
    HandlingOutcomeKind, acknowledge, quarantine, resolve_handler_failure, resolve_store_failure,
};
use crate::{HandlerError, HandlerPolicy, HandlingOutcome, MessageFailure};

/// Process and settle exactly one delivery.
///
/// The supplied store owns the atomic inbox and service transition. Broker
/// acknowledgement follows its committed result. Permanent failures commit
/// quarantine evidence before terminal settlement. Transient failures request
/// deterministic delayed redelivery; infrastructure failures are never
/// converted into terminal success.
///
/// # Errors
///
/// Returns a configuration, invariant, quarantine, or broker-settlement failure.
/// The owned delivery is dropped without successful settlement on error.
pub async fn handle_once(
    store: &mut dyn InboundMessageStore,
    registry: &MessageRegistry<'_>,
    policy: &HandlerPolicy,
    delivery: MessageDelivery,
) -> Result<HandlingOutcome, HandlerError> {
    let decoded_envelope = MessageEnvelope::from_json(delivery.payload());
    let recorder = SpineRecorder::new(delivery.metadata(), &decoded_envelope);
    let result = handle_decoded_once(
        store,
        registry,
        policy,
        delivery,
        decoded_envelope,
        &recorder,
    )
    .await;
    recorder.finish(result)
}

async fn handle_decoded_once(
    store: &mut dyn InboundMessageStore,
    registry: &MessageRegistry<'_>,
    policy: &HandlerPolicy,
    delivery: MessageDelivery,
    decoded_envelope: Result<MessageEnvelope, MessageContractError>,
    recorder: &SpineRecorder,
) -> Result<HandlingOutcome, HandlerError> {
    let envelope = match decoded_envelope {
        Ok(envelope) => envelope,
        Err(error) => {
            return quarantine(
                store,
                policy,
                delivery,
                recorder,
                "envelope_invalid",
                MessageFailure::Envelope(error),
            )
            .await;
        }
    };
    if let Err(error) = registry.validate(&envelope) {
        return quarantine(
            store,
            policy,
            delivery,
            recorder,
            "routing_invalid",
            MessageFailure::Routing(error),
        )
        .await;
    }

    let persistence_started = recorder.start_stage();
    let processing_result = store
        .process(&policy.consumer_name, registry, &envelope)
        .await;
    recorder.record_processing_result(persistence_started, &processing_result);
    match processing_result {
        Ok(InboxDisposition::Duplicate) => {
            acknowledge(delivery, recorder, HandlingOutcomeKind::Duplicate).await
        }
        Ok(InboxDisposition::Applied) => {
            acknowledge(delivery, recorder, HandlingOutcomeKind::Applied).await
        }
        Err(InboundProcessingError::Handler(failure)) => {
            resolve_handler_failure(store, policy, delivery, recorder, failure).await
        }
        Err(InboundProcessingError::Store(error)) => {
            resolve_store_failure(store, policy, delivery, recorder, error).await
        }
    }
}
