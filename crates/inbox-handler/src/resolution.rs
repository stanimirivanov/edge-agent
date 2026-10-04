//! Final delivery actions after routing or atomic inbound processing decides an outcome.

use crate::observability::SpineRecorder;
use crate::policy::validate_failure_code;
use crate::retry::{retry_delay, should_retry_handler};
use crate::{HandlerError, HandlerPolicy, HandlingOutcome, MessageFailure};
use edgeagent_messaging::{
    DeliveryDisposition as SettlementDisposition, HandlerFailure, InboundMessageStore,
    InboundQuarantine, InboxDisposition, InboxStoreError, InboxStoreErrorKind, MessageDelivery,
    RetryDelay,
};

pub(super) async fn acknowledge(
    delivery: MessageDelivery,
    recorder: &SpineRecorder,
    disposition: InboxDisposition,
) -> Result<HandlingOutcome, HandlerError> {
    let attempt = delivery.metadata().delivery_attempt();
    settle_delivery(delivery, SettlementDisposition::Acknowledge, recorder).await?;
    Ok(match disposition {
        InboxDisposition::Applied => HandlingOutcome::Applied { attempt },
        InboxDisposition::Duplicate => HandlingOutcome::Duplicate { attempt },
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

pub(super) async fn resolve_store_failure(
    store: &mut dyn InboundMessageStore,
    policy: &HandlerPolicy,
    delivery: MessageDelivery,
    recorder: &SpineRecorder,
    error: InboxStoreError,
) -> Result<HandlingOutcome, HandlerError> {
    let failure_code = match error.kind() {
        InboxStoreErrorKind::Contract => "routing_invalid",
        InboxStoreErrorKind::MessageIdentityConflict => "message_identity_conflict",
        InboxStoreErrorKind::Unavailable => {
            return retry(delivery, policy, recorder, MessageFailure::Inbox(error)).await;
        }
        InboxStoreErrorKind::Invariant => return Err(HandlerError::inbox(error)),
    };
    quarantine(
        store,
        policy,
        delivery,
        recorder,
        failure_code,
        MessageFailure::Inbox(error),
    )
    .await
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
    settle_delivery(delivery, SettlementDisposition::RetryAfter(delay), recorder).await?;
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
    let evidence = InboundQuarantine::new(
        delivery.metadata().message_key(),
        delivery.metadata().subject(),
        delivery.metadata().delivery_attempt(),
        delivery.payload(),
        failure_code,
    )
    .map_err(HandlerError::inbox)?;
    let persistence_started = recorder.start_stage();
    let quarantine_result = store.quarantine(&policy.consumer_name, evidence).await;
    recorder.record_quarantine_result(persistence_started, &quarantine_result);
    let disposition = match quarantine_result {
        Ok(disposition) => disposition,
        Err(error) if error.kind() == InboxStoreErrorKind::Unavailable => {
            return retry(delivery, policy, recorder, MessageFailure::Inbox(error)).await;
        }
        Err(error) => return Err(HandlerError::inbox(error)),
    };
    let attempt = delivery.metadata().delivery_attempt();
    settle_delivery(delivery, SettlementDisposition::Quarantined, recorder).await?;
    Ok(HandlingOutcome::Quarantined {
        attempt,
        disposition,
        failure_code,
        failure,
    })
}

/// Wait for broker confirmation and record the classified settlement result.
async fn settle_delivery(
    delivery: MessageDelivery,
    disposition: SettlementDisposition,
    recorder: &SpineRecorder,
) -> Result<(), HandlerError> {
    let started = recorder.start_stage();
    let result = delivery.settle(disposition).await;
    recorder.record_settlement_result(started, disposition, &result);
    result.map_err(HandlerError::settlement)
}
