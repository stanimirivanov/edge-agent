//! Final delivery actions after routing or atomic inbound processing decides an outcome.

use edgeagent_messaging::{
    DeliveryDisposition as SettlementDisposition, HandlerFailure, InboundMessageStore,
    InboundQuarantine, InboxStoreErrorKind, MessageDelivery, RetryDelay,
};
use edgeagent_telemetry::{
    EventSpineContext, EventSpineOutcome, EventSpineStage, record_event_spine_operation,
};
use std::time::Instant;

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
    telemetry_context: &EventSpineContext,
    outcome: HandlingOutcomeKind,
) -> Result<HandlingOutcome, HandlerError> {
    let attempt = delivery.metadata().delivery_attempt();
    let started = Instant::now();
    let settlement_result = delivery.settle(SettlementDisposition::Acknowledge).await;
    record_acknowledgement(
        telemetry_context,
        attempt,
        if settlement_result.is_ok() {
            EventSpineOutcome::Succeeded
        } else {
            EventSpineOutcome::Failed
        },
        started,
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
    telemetry_context: &EventSpineContext,
    failure: HandlerFailure,
) -> Result<HandlingOutcome, HandlerError> {
    validate_failure_code(failure.code())?;
    if should_retry_handler(
        policy,
        delivery.metadata().delivery_attempt(),
        failure.kind(),
    ) {
        retry(
            delivery,
            policy,
            telemetry_context,
            MessageFailure::Handler(failure),
        )
        .await
    } else {
        let code = failure.code();
        quarantine(
            store,
            policy,
            delivery,
            telemetry_context,
            code,
            MessageFailure::Handler(failure),
        )
        .await
    }
}

pub(super) async fn retry(
    delivery: MessageDelivery,
    policy: &HandlerPolicy,
    telemetry_context: &EventSpineContext,
    failure: MessageFailure,
) -> Result<HandlingOutcome, HandlerError> {
    let attempt = delivery.metadata().delivery_attempt();
    let delay = RetryDelay::new(retry_delay(
        policy,
        delivery.metadata().message_key(),
        attempt,
    ))
    .map_err(HandlerError::settlement)?;
    let started = Instant::now();
    let settlement_result = delivery
        .settle(SettlementDisposition::RetryAfter(delay))
        .await;
    record_acknowledgement(
        telemetry_context,
        attempt,
        if settlement_result.is_ok() {
            EventSpineOutcome::RetryScheduled
        } else {
            EventSpineOutcome::Failed
        },
        started,
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
    telemetry_context: &EventSpineContext,
    failure_code: &'static str,
    failure: MessageFailure,
) -> Result<HandlingOutcome, HandlerError> {
    validate_failure_code(failure_code)?;
    let persistence_started = Instant::now();
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
            record_persistence(
                telemetry_context,
                delivery.metadata().delivery_attempt(),
                EventSpineOutcome::Failed,
                persistence_started,
            );
            return retry(
                delivery,
                policy,
                telemetry_context,
                MessageFailure::Inbox(error),
            )
            .await;
        }
        Err(error) => {
            record_persistence(
                telemetry_context,
                delivery.metadata().delivery_attempt(),
                EventSpineOutcome::Failed,
                persistence_started,
            );
            return Err(HandlerError::inbox(error));
        }
    };
    let attempt = delivery.metadata().delivery_attempt();
    record_persistence(
        telemetry_context,
        attempt,
        EventSpineOutcome::Quarantined,
        persistence_started,
    );
    let acknowledgement_started = Instant::now();
    let settlement_result = delivery.settle(SettlementDisposition::Quarantined).await;
    record_acknowledgement(
        telemetry_context,
        attempt,
        if settlement_result.is_ok() {
            EventSpineOutcome::Quarantined
        } else {
            EventSpineOutcome::Failed
        },
        acknowledgement_started,
    );
    settlement_result.map_err(HandlerError::settlement)?;
    Ok(HandlingOutcome::Quarantined {
        attempt,
        disposition,
        failure_code,
        failure,
    })
}

pub(super) fn record_persistence(
    telemetry_context: &EventSpineContext,
    attempt: u32,
    outcome: EventSpineOutcome,
    started: Instant,
) {
    record_event_spine_operation(
        telemetry_context,
        EventSpineStage::Persistence,
        outcome,
        attempt,
        started.elapsed(),
    );
}

fn record_acknowledgement(
    telemetry_context: &EventSpineContext,
    attempt: u32,
    outcome: EventSpineOutcome,
    started: Instant,
) {
    record_event_spine_operation(
        telemetry_context,
        EventSpineStage::Acknowledgement,
        outcome,
        attempt,
        started.elapsed(),
    );
}
