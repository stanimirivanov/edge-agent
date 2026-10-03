use edgeagent_contracts::{MessageContractError, MessageEnvelope, MessageRegistry};
use edgeagent_messaging::{
    InboundMessageStore, InboundProcessingError, InboxDisposition, InboxStoreErrorKind,
    MessageDelivery,
};
use edgeagent_telemetry::{
    EventSpineContext, EventSpineOutcome, EventSpineStage, record_event_spine_operation,
};
use std::time::Instant;

use crate::resolution::{
    HandlingOutcomeKind, acknowledge, quarantine, record_persistence, resolve_handler_failure,
    retry,
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
    let attempt = delivery.metadata().delivery_attempt();
    let decoded_envelope = MessageEnvelope::from_json(delivery.payload());
    let telemetry_context = match &decoded_envelope {
        Ok(envelope) => EventSpineContext::from_envelope(envelope),
        Err(_) => EventSpineContext::from_delivery(delivery.metadata()),
    };
    let handling_started = Instant::now();
    let result = handle_decoded_once(
        store,
        registry,
        policy,
        delivery,
        decoded_envelope,
        &telemetry_context,
    )
    .await;
    let outcome = match &result {
        Ok(HandlingOutcome::Applied { .. }) => EventSpineOutcome::Succeeded,
        Ok(HandlingOutcome::Duplicate { .. }) => EventSpineOutcome::Duplicate,
        Ok(HandlingOutcome::RetryRequested { .. }) => EventSpineOutcome::RetryScheduled,
        Ok(HandlingOutcome::Quarantined { .. }) => EventSpineOutcome::Quarantined,
        Err(_) => EventSpineOutcome::Failed,
    };
    record_event_spine_operation(
        &telemetry_context,
        EventSpineStage::Handling,
        outcome,
        attempt,
        handling_started.elapsed(),
    );
    result
}

async fn handle_decoded_once(
    store: &mut dyn InboundMessageStore,
    registry: &MessageRegistry<'_>,
    policy: &HandlerPolicy,
    delivery: MessageDelivery,
    decoded_envelope: Result<MessageEnvelope, MessageContractError>,
    telemetry_context: &EventSpineContext,
) -> Result<HandlingOutcome, HandlerError> {
    let envelope = match decoded_envelope {
        Ok(envelope) => envelope,
        Err(error) => {
            return quarantine(
                store,
                policy,
                delivery,
                telemetry_context,
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
            telemetry_context,
            "routing_invalid",
            MessageFailure::Routing(error),
        )
        .await;
    }

    let persistence_started = Instant::now();
    match store
        .process(&policy.consumer_name, registry, &envelope)
        .await
    {
        Ok(InboxDisposition::Duplicate) => {
            record_persistence(
                telemetry_context,
                delivery.metadata().delivery_attempt(),
                EventSpineOutcome::Duplicate,
                persistence_started,
            );
            acknowledge(delivery, telemetry_context, HandlingOutcomeKind::Duplicate).await
        }
        Ok(InboxDisposition::Applied) => {
            record_persistence(
                telemetry_context,
                delivery.metadata().delivery_attempt(),
                EventSpineOutcome::Succeeded,
                persistence_started,
            );
            acknowledge(delivery, telemetry_context, HandlingOutcomeKind::Applied).await
        }
        Err(InboundProcessingError::Handler(failure)) => {
            resolve_handler_failure(store, policy, delivery, telemetry_context, failure).await
        }
        Err(InboundProcessingError::Store(error)) => {
            record_persistence(
                telemetry_context,
                delivery.metadata().delivery_attempt(),
                EventSpineOutcome::Failed,
                persistence_started,
            );
            match error.kind() {
                InboxStoreErrorKind::Contract => {
                    quarantine(
                        store,
                        policy,
                        delivery,
                        telemetry_context,
                        "routing_invalid",
                        MessageFailure::Inbox(error),
                    )
                    .await
                }
                InboxStoreErrorKind::MessageIdentityConflict => {
                    quarantine(
                        store,
                        policy,
                        delivery,
                        telemetry_context,
                        "message_identity_conflict",
                        MessageFailure::Inbox(error),
                    )
                    .await
                }
                InboxStoreErrorKind::Unavailable => {
                    retry(
                        delivery,
                        policy,
                        telemetry_context,
                        MessageFailure::Inbox(error),
                    )
                    .await
                }
                InboxStoreErrorKind::Invariant => Err(HandlerError::inbox(error)),
            }
        }
    }
}
