//! One-record relay coordination across portable publisher and store ports.

use crate::retry::{RetryDecision, retry_decision};
use crate::{QuarantineReason, RelayError, RelayMessageFailure, RelayOutcome, RelayPolicy};
use edgeagent_contracts::MessageRegistry;
use edgeagent_messaging::{
    ClaimedMessage, MessagePublisher, OutboxRelayStore, OutboxRetryDelay, PublishDisposition,
    PublishError,
};
use edgeagent_telemetry::{
    EventSpineContext, EventSpineOutcome, EventSpineStage, record_event_spine_operation,
};
use std::time::Instant;

/// Claim and resolve at most one outbox record.
///
/// Publication occurs outside persistence transactions. Each worker has at most
/// one in-flight publication, providing an explicit backpressure boundary.
/// Broker confirmation, retry scheduling, or quarantine is then committed in a
/// short transaction guarded by the original lease.
///
/// # Cancellation
///
/// Cancellation never schedules a replacement storage transition. A claim or
/// completion may already have committed, and a publication may already have
/// persisted without a receipt. Resolve through actual storage state and normal
/// lease expiry/reacquisition; preserve message identity and immutable bytes.
/// Every later completion still requires its own current claim generation.
/// Fencing protects storage, not a publication already sent to the broker.
///
/// # Errors
///
/// Returns a storage or outbox error when claiming or durably recording the
/// outcome fails. A publish failure is a normal classified `RelayOutcome`.
pub async fn relay_once(
    store: &mut dyn OutboxRelayStore,
    registry: &MessageRegistry<'_>,
    publisher: &dyn MessagePublisher,
    policy: &RelayPolicy,
) -> Result<RelayOutcome, RelayError> {
    let Some(message) = store
        .claim_one(&policy.lease_owner, policy.lease_duration)
        .await?
    else {
        return Ok(RelayOutcome::Idle);
    };

    let mut telemetry_context = EventSpineContext::from_stored_identity(
        message.message_source(),
        message.message_id(),
        message.message_type(),
    );
    let envelope = match message.validated_envelope(registry) {
        Ok(envelope) => {
            telemetry_context = EventSpineContext::from_envelope(&envelope);
            envelope
        }
        Err(error) => {
            return quarantine(
                store,
                policy,
                &message,
                &telemetry_context,
                QuarantineReason::StoredContractInvalid,
                RelayMessageFailure::StoredContract(error),
            )
            .await;
        }
    };
    let definition = match registry.validate(&envelope) {
        Ok(definition) => *definition,
        Err(error) => {
            return quarantine(
                store,
                policy,
                &message,
                &telemetry_context,
                QuarantineReason::StoredContractInvalid,
                RelayMessageFailure::StoredContract(error),
            )
            .await;
        }
    };

    let publication_started = Instant::now();
    match publisher.publish(definition, &envelope).await {
        Ok(receipt) => {
            let disposition = receipt.disposition();
            record_event_spine_operation(
                &telemetry_context,
                EventSpineStage::Publication,
                match disposition {
                    PublishDisposition::Persisted => EventSpineOutcome::Succeeded,
                    PublishDisposition::Duplicate => EventSpineOutcome::Duplicate,
                },
                message.attempt(),
                publication_started.elapsed(),
            );
            let persistence_started = Instant::now();
            let persistence_result = store
                .mark_published(&message, &policy.lease_owner)
                .await
                .map_err(RelayError::from);
            record_event_spine_operation(
                &telemetry_context,
                EventSpineStage::Persistence,
                if persistence_result.is_ok() {
                    EventSpineOutcome::Succeeded
                } else {
                    EventSpineOutcome::Failed
                },
                message.attempt(),
                persistence_started.elapsed(),
            );
            persistence_result?;
            Ok(RelayOutcome::Published {
                disposition,
                attempt: message.attempt(),
            })
        }
        Err(error) => {
            record_event_spine_operation(
                &telemetry_context,
                EventSpineStage::Publication,
                EventSpineOutcome::Failed,
                message.attempt(),
                publication_started.elapsed(),
            );
            resolve_failure(store, policy, &message, &telemetry_context, error).await
        }
    }
}

async fn resolve_failure(
    store: &mut dyn OutboxRelayStore,
    policy: &RelayPolicy,
    message: &ClaimedMessage,
    telemetry_context: &EventSpineContext,
    failure: PublishError,
) -> Result<RelayOutcome, RelayError> {
    let failure_kind = failure.kind();
    match retry_decision(
        policy,
        message.message_source(),
        message.message_id(),
        message.attempt(),
        failure_kind,
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
            let persistence_started = Instant::now();
            let persistence_result = store
                .release_for_retry(message, &policy.lease_owner, retry_after, failure_code)
                .await
                .map_err(RelayError::from);
            record_event_spine_operation(
                telemetry_context,
                EventSpineStage::Persistence,
                if persistence_result.is_ok() {
                    EventSpineOutcome::RetryScheduled
                } else {
                    EventSpineOutcome::Failed
                },
                message.attempt(),
                persistence_started.elapsed(),
            );
            persistence_result?;
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
                telemetry_context,
                reason,
                RelayMessageFailure::Publication(failure),
            )
            .await
        }
    }
}

async fn quarantine(
    store: &mut dyn OutboxRelayStore,
    policy: &RelayPolicy,
    message: &ClaimedMessage,
    telemetry_context: &EventSpineContext,
    reason: QuarantineReason,
    failure: RelayMessageFailure,
) -> Result<RelayOutcome, RelayError> {
    let persistence_started = Instant::now();
    let persistence_result = store
        .quarantine(message, &policy.lease_owner, reason.code())
        .await
        .map_err(RelayError::from);
    record_event_spine_operation(
        telemetry_context,
        EventSpineStage::Persistence,
        if persistence_result.is_ok() {
            EventSpineOutcome::Quarantined
        } else {
            EventSpineOutcome::Failed
        },
        message.attempt(),
        persistence_started.elapsed(),
    );
    persistence_result?;
    Ok(RelayOutcome::Quarantined {
        reason,
        attempt: message.attempt(),
        failure,
    })
}
