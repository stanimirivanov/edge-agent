//! Durable outcome resolution under the original claim, without alternate recovery.

use crate::observation::RelayObservation;
use crate::retry::{RetryDecision, retry_decision};
use crate::{QuarantineReason, RelayError, RelayMessageFailure, RelayOutcome, RelayPolicy};
use edgeagent_messaging::{
    ClaimedMessage, OutboxRelayStore, OutboxRetryDelay, PublishDisposition, PublishError,
};
use edgeagent_telemetry::EventSpineOutcome;

pub(super) async fn published(
    store: &mut dyn OutboxRelayStore,
    policy: &RelayPolicy,
    message: &ClaimedMessage,
    observation: &RelayObservation,
    disposition: PublishDisposition,
) -> Result<RelayOutcome, RelayError> {
    let started = observation.start_stage();
    let result = store.mark_published(message, &policy.lease_owner).await;
    observation.persistence(started, &result, EventSpineOutcome::Succeeded);
    result?;
    Ok(RelayOutcome::Published {
        disposition,
        attempt: message.attempt(),
    })
}

pub(super) async fn failed(
    store: &mut dyn OutboxRelayStore,
    policy: &RelayPolicy,
    message: &ClaimedMessage,
    observation: &RelayObservation,
    failure: PublishError,
) -> Result<RelayOutcome, RelayError> {
    match retry_decision(
        policy,
        message.message_source(),
        message.message_id(),
        message.attempt(),
        failure.kind(),
    ) {
        RetryDecision::Retry {
            delay,
            failure_code,
        } => {
            let retry_after = OutboxRetryDelay::new(delay).map_err(|_| {
                RelayError::invalid_policy(
                    "calculated retry delay exceeds the outbox timing bounds",
                )
            })?;
            let started = observation.start_stage();
            let result = store
                .release_for_retry(message, &policy.lease_owner, retry_after, failure_code)
                .await;
            observation.persistence(started, &result, EventSpineOutcome::RetryScheduled);
            result?;
            Ok(RelayOutcome::RetryScheduled {
                failure,
                attempt: message.attempt(),
                delay,
            })
        }
        RetryDecision::Quarantine(reason) => {
            quarantine(
                store,
                policy,
                message,
                observation,
                reason,
                RelayMessageFailure::Publication(failure),
            )
            .await
        }
    }
}

pub(super) async fn quarantine(
    store: &mut dyn OutboxRelayStore,
    policy: &RelayPolicy,
    message: &ClaimedMessage,
    observation: &RelayObservation,
    reason: QuarantineReason,
    failure: RelayMessageFailure,
) -> Result<RelayOutcome, RelayError> {
    let started = observation.start_stage();
    let result = store
        .quarantine(message, &policy.lease_owner, reason.code())
        .await;
    observation.persistence(started, &result, EventSpineOutcome::Quarantined);
    result?;
    Ok(RelayOutcome::Quarantined {
        reason,
        attempt: message.attempt(),
        failure,
    })
}
