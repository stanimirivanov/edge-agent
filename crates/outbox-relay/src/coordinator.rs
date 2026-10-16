//! One-record relay coordination across portable publisher and store ports.

use crate::observation::RelayObservation;
use crate::resolution;
use crate::{QuarantineReason, RelayError, RelayMessageFailure, RelayOutcome, RelayPolicy};
use edgeagent_contracts::{MessageRegistry, MessageRoutingError};
use edgeagent_messaging::{MessagePublisher, OutboxRelayStore};

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

    let envelope = match message.validated_envelope(registry) {
        Ok(envelope) => envelope,
        Err(error) => {
            // No persisted identity is trusted until the entire record validates.
            let observation = RelayObservation::rejected(message.attempt());
            return resolution::quarantine(
                store,
                policy,
                &message,
                &observation,
                QuarantineReason::StoredContractInvalid,
                RelayMessageFailure::StoredContract(error),
            )
            .await;
        }
    };
    // validated_envelope already checked this immutable registry and routing policy.
    // Resolve metadata without repeating envelope validation; fail closed if absent.
    let Some(&definition) = registry.resolve(envelope.message_type()) else {
        let observation = RelayObservation::rejected(message.attempt());
        return resolution::quarantine(
            store,
            policy,
            &message,
            &observation,
            QuarantineReason::StoredContractInvalid,
            RelayMessageFailure::StoredContract(MessageRoutingError::UnsupportedMessageType),
        )
        .await;
    };
    let observation = RelayObservation::validated(&envelope, message.attempt());
    let started = observation.start_stage();
    let publication = publisher.publish(definition, &envelope).await;
    observation.publication(started, &publication);

    match publication {
        Ok(receipt) => {
            resolution::published(store, policy, &message, &observation, receipt.disposition())
                .await
        }
        Err(error) => resolution::failed(store, policy, &message, &observation, error).await,
    }
}
