//! Final delivery actions after routing or atomic inbound processing decides an outcome.

use edgeagent_messaging::{
    DeliveryDisposition as SettlementDisposition, HandlerFailure, InboundMessageStore,
    InboundQuarantine, InboxStoreErrorKind, MessageDelivery, RetryDelay,
};
use edgeagent_telemetry::EventSpineOutcome;

use crate::observability::SpineRecorder;
use crate::policy::validate_failure_code;
use crate::retry::{retry_delay, should_retry_handler};
use crate::{HandlerError, HandlerPolicy, HandlingOutcome, MessageFailure};

#[derive(Clone, Copy)]
pub(super) enum HandlingOutcomeKind {
    Applied,
    Duplicate,
}

pub(super) async fn acknowledge(
    delivery: MessageDelivery,
    recorder: &SpineRecorder,
    outcome: HandlingOutcomeKind,
) -> Result<HandlingOutcome, HandlerError> {
    let attempt = delivery.metadata().delivery_attempt();
    let started = recorder.start_stage();
    let settlement_result = delivery.settle(SettlementDisposition::Acknowledge).await;
    recorder.record_acknowledgement(
        started,
        if settlement_result.is_ok() {
            EventSpineOutcome::Succeeded
        } else {
            EventSpineOutcome::Failed
        },
    );
    settlement_result.map_err(HandlerError::settlement)?;
    Ok(match outcome {
        HandlingOutcomeKind::Applied => HandlingOutcome::Applied { attempt },
        HandlingOutcomeKind::Duplicate => HandlingOutcome::Duplicate { attempt },
    })
}

pub(super) async fn resolve_handler_failure(
    store: &mut dyn InboundMessageStore,
    policy: &HandlerPolicy,
    delivery: MessageDelivery,
    recorder: &SpineRecorder,
    failure: HandlerFailure,
) -> Result<HandlingOutcome, HandlerError> {
    validate_failure_code(failure.code())?;
    if should_retry_handler(
        policy,
        delivery.metadata().delivery_attempt(),
        failure.kind(),
    ) {
        retry(delivery, policy, recorder, MessageFailure::Handler(failure)).await
    } else {
        let code = failure.code();
        quarantine(
            store,
            policy,
            delivery,
            recorder,
            code,
            MessageFailure::Handler(failure),
        )
        .await
    }
}

pub(super) async fn retry(
    delivery: MessageDelivery,
    policy: &HandlerPolicy,
    recorder: &SpineRecorder,
    failure: MessageFailure,
) -> Result<HandlingOutcome, HandlerError> {
    let attempt = delivery.metadata().delivery_attempt();
    let delay = RetryDelay::new(retry_delay(
        policy,
        delivery.metadata().message_key(),
        attempt,
    ))
    .map_err(HandlerError::settlement)?;
    let started = recorder.start_stage();
    let settlement_result = delivery
        .settle(SettlementDisposition::RetryAfter(delay))
        .await;
    recorder.record_acknowledgement(
        started,
        if settlement_result.is_ok() {
            EventSpineOutcome::RetryScheduled
        } else {
            EventSpineOutcome::Failed
        },
    );
    settlement_result.map_err(HandlerError::settlement)?;
    Ok(HandlingOutcome::RetryRequested {
        attempt,
        delay: delay.get(),
        failure,
    })
}

pub(super) async fn quarantine(
    store: &mut dyn InboundMessageStore,
    policy: &HandlerPolicy,
    delivery: MessageDelivery,
    recorder: &SpineRecorder,
    failure_code: &'static str,
    failure: MessageFailure,
) -> Result<HandlingOutcome, HandlerError> {
    validate_failure_code(failure_code)?;
    let persistence_started = recorder.start_stage();
    let quarantine_result = {
        let evidence = InboundQuarantine::new(
            delivery.metadata().message_key(),
            delivery.metadata().subject(),
            delivery.metadata().delivery_attempt(),
            delivery.payload(),
            failure_code,
        )
        .map_err(HandlerError::inbox)?;
        store.quarantine(&policy.consumer_name, evidence).await
    };
    let disposition = match quarantine_result {
        Ok(disposition) => disposition,
        Err(error) if error.kind() == InboxStoreErrorKind::Unavailable => {
            recorder.record_persistence(persistence_started, EventSpineOutcome::Failed);
            return retry(delivery, policy, recorder, MessageFailure::Inbox(error)).await;
        }
        Err(error) => {
            recorder.record_persistence(persistence_started, EventSpineOutcome::Failed);
            return Err(HandlerError::inbox(error));
        }
    };
    let attempt = delivery.metadata().delivery_attempt();
    recorder.record_persistence(persistence_started, EventSpineOutcome::Quarantined);
    let acknowledgement_started = recorder.start_stage();
    let settlement_result = delivery.settle(SettlementDisposition::Quarantined).await;
    recorder.record_acknowledgement(
        acknowledgement_started,
        if settlement_result.is_ok() {
            EventSpineOutcome::Quarantined
        } else {
            EventSpineOutcome::Failed
        },
    );
    settlement_result.map_err(HandlerError::settlement)?;
    Ok(HandlingOutcome::Quarantined {
        attempt,
        disposition,
        failure_code,
        failure,
    })
}
