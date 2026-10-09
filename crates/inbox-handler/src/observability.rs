//! One-delivery event-spine recording without affecting resolution policy.

use edgeagent_contracts::{MessageContractError, MessageEnvelope};
use edgeagent_messaging::{
    ConsumeError, DeliveryDisposition, DeliveryMetadata, InboundProcessingError, InboxDisposition,
    InboxStoreError, QuarantineDisposition,
};
use edgeagent_telemetry::{
    EventSpineContext, EventSpineOutcome, EventSpineStage, record_event_spine_operation,
};
use std::time::Instant;

use crate::{HandlerError, HandlingOutcome};

/// An explicit stage timer that is consumed when its result is recorded.
pub(super) struct StageStart(Instant);

/// Approved correlation context and timing for one handled delivery.
pub(super) struct SpineRecorder {
    context: EventSpineContext,
    attempt: u32,
    handling_started: Instant,
}

impl SpineRecorder {
    /// Start handling only after decoding and context selection, as before.
    pub(super) fn new(
        metadata: &DeliveryMetadata,
        decoded_envelope: &Result<MessageEnvelope, MessageContractError>,
    ) -> Self {
        let context = match decoded_envelope {
            Ok(envelope) => EventSpineContext::from_envelope(envelope),
            Err(_) => EventSpineContext::from_delivery(metadata),
        };
        Self {
            context,
            attempt: metadata.delivery_attempt(),
            handling_started: Instant::now(),
        }
    }

    pub(super) fn start_stage(&self) -> StageStart {
        StageStart(Instant::now())
    }

    fn record_persistence(&self, started: StageStart, outcome: EventSpineOutcome) {
        self.record(EventSpineStage::Persistence, outcome, started.0);
    }

    pub(super) fn record_processing_result(
        &self,
        started: StageStart,
        result: &Result<InboxDisposition, InboundProcessingError>,
    ) {
        self.record_persistence(started, processing_outcome(result));
    }

    pub(super) fn record_quarantine_result(
        &self,
        started: StageStart,
        result: &Result<QuarantineDisposition, InboxStoreError>,
    ) {
        self.record_persistence(started, quarantine_outcome(result));
    }

    pub(super) fn record_settlement_result(
        &self,
        started: StageStart,
        disposition: DeliveryDisposition,
        result: &Result<(), ConsumeError>,
    ) {
        self.record(
            EventSpineStage::Acknowledgement,
            settlement_outcome(disposition, result),
            started.0,
        );
    }

    pub(super) fn finish(
        self,
        result: Result<HandlingOutcome, HandlerError>,
    ) -> Result<HandlingOutcome, HandlerError> {
        let outcome = handling_outcome(&result);
        self.record(EventSpineStage::Handling, outcome, self.handling_started);
        result
    }

    fn record(&self, stage: EventSpineStage, outcome: EventSpineOutcome, started: Instant) {
        record_event_spine_operation(
            &self.context,
            stage,
            outcome,
            self.attempt,
            started.elapsed(),
        );
    }
}

fn handling_outcome(result: &Result<HandlingOutcome, HandlerError>) -> EventSpineOutcome {
    match result {
        Ok(HandlingOutcome::Applied { .. }) => EventSpineOutcome::Succeeded,
        Ok(HandlingOutcome::Duplicate { .. }) => EventSpineOutcome::Duplicate,
        Ok(HandlingOutcome::RetryRequested { .. }) => EventSpineOutcome::RetryScheduled,
        Ok(HandlingOutcome::Quarantined { .. }) => EventSpineOutcome::Quarantined,
        Err(_) => EventSpineOutcome::Failed,
    }
}

fn processing_outcome(
    result: &Result<InboxDisposition, InboundProcessingError>,
) -> EventSpineOutcome {
    match result {
        Ok(InboxDisposition::Applied) => EventSpineOutcome::Succeeded,
        Ok(InboxDisposition::Duplicate) => EventSpineOutcome::Duplicate,
        Err(_) => EventSpineOutcome::Failed,
    }
}

fn quarantine_outcome(
    result: &Result<QuarantineDisposition, InboxStoreError>,
) -> EventSpineOutcome {
    match result {
        Ok(QuarantineDisposition::Inserted | QuarantineDisposition::AlreadyPresent) => {
            EventSpineOutcome::Quarantined
        }
        Err(_) => EventSpineOutcome::Failed,
    }
}

fn settlement_outcome(
    disposition: DeliveryDisposition,
    result: &Result<(), ConsumeError>,
) -> EventSpineOutcome {
    if result.is_err() {
        return EventSpineOutcome::Failed;
    }
    match disposition {
        DeliveryDisposition::Acknowledge => EventSpineOutcome::Succeeded,
        DeliveryDisposition::RetryAfter(_) => EventSpineOutcome::RetryScheduled,
        DeliveryDisposition::Quarantined => EventSpineOutcome::Quarantined,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SpineRecorder, handling_outcome, processing_outcome, quarantine_outcome, settlement_outcome,
    };
    use crate::{HandlerError, HandlingOutcome, MessageFailure};
    use edgeagent_contracts::MessageEnvelope;
    use edgeagent_messaging::{
        ConsumeError, DeliveryAttempt, DeliveryDisposition, DeliveryMessageKey, DeliveryMetadata,
        DeliverySubject, FailureCode, HandlerFailure, InboundProcessingError, InboxDisposition,
        InboxStoreError, InboxStoreErrorKind, QuarantineDisposition, RetryDelay,
    };
    use edgeagent_telemetry::EventSpineOutcome;
    use std::error::Error;
    use std::io;
    use std::time::Duration;

    #[test]
    fn context_uses_validated_envelope_or_opaque_delivery_identity() -> Result<(), Box<dyn Error>> {
        let metadata = DeliveryMetadata::new(
            DeliveryMessageKey::new("stream:41")?,
            DeliverySubject::new("edgeagent.command.research.request-received.v1")?,
            DeliveryAttempt::new(3)?,
        );
        let valid = MessageEnvelope::from_json(include_bytes!(
            "../../contracts/fixtures/v1/research-request-received.json"
        ));
        let valid_recorder = SpineRecorder::new(&metadata, &valid);
        assert_eq!(valid_recorder.context.message_id(), Some("message-01"));
        assert_eq!(valid_recorder.context.transport_message_key(), None);
        assert_eq!(valid_recorder.attempt, 3);

        let invalid = MessageEnvelope::from_json(b"{not-json");
        let invalid_recorder = SpineRecorder::new(&metadata, &invalid);
        assert_eq!(invalid_recorder.context.message_id(), None);
        assert_eq!(
            invalid_recorder.context.transport_message_key(),
            Some("stream:41")
        );
        assert_eq!(invalid_recorder.attempt, 3);
        Ok(())
    }

    #[test]
    fn final_handling_outcome_matches_confirmed_result() {
        let cases = [
            (
                Ok(HandlingOutcome::Applied { attempt: 1 }),
                EventSpineOutcome::Succeeded,
            ),
            (
                Ok(HandlingOutcome::Duplicate { attempt: 2 }),
                EventSpineOutcome::Duplicate,
            ),
            (
                Ok(HandlingOutcome::RetryRequested {
                    attempt: 1,
                    delay: Duration::from_millis(1),
                    failure: MessageFailure::Handler(HandlerFailure::transient(
                        FailureCode::from_static("retry"),
                    )),
                }),
                EventSpineOutcome::RetryScheduled,
            ),
            (
                Ok(HandlingOutcome::Quarantined {
                    attempt: 3,
                    disposition: QuarantineDisposition::Inserted,
                    failure_code: "rejected",
                    failure: MessageFailure::Handler(HandlerFailure::permanent(
                        FailureCode::from_static("rejected"),
                    )),
                }),
                EventSpineOutcome::Quarantined,
            ),
            (
                Err(HandlerError::invalid_policy("invalid")),
                EventSpineOutcome::Failed,
            ),
        ];
        for (result, expected) in cases {
            assert_eq!(handling_outcome(&result), expected);
        }
    }

    #[test]
    fn processing_result_always_has_a_persistence_outcome() {
        let cases = [
            (Ok(InboxDisposition::Applied), EventSpineOutcome::Succeeded),
            (
                Ok(InboxDisposition::Duplicate),
                EventSpineOutcome::Duplicate,
            ),
            (
                Err(InboundProcessingError::Handler(HandlerFailure::transient(
                    FailureCode::from_static("retry"),
                ))),
                EventSpineOutcome::Failed,
            ),
            (
                Err(InboundProcessingError::Store(InboxStoreError::with_source(
                    InboxStoreErrorKind::Unavailable,
                    io::Error::other("test storage outage"),
                ))),
                EventSpineOutcome::Failed,
            ),
        ];
        for (result, expected) in cases {
            assert_eq!(processing_outcome(&result), expected);
        }
    }

    #[test]
    fn quarantine_result_always_has_a_persistence_outcome() {
        let cases = [
            (
                Ok(QuarantineDisposition::Inserted),
                EventSpineOutcome::Quarantined,
            ),
            (
                Ok(QuarantineDisposition::AlreadyPresent),
                EventSpineOutcome::Quarantined,
            ),
            (
                Err(InboxStoreError::with_source(
                    InboxStoreErrorKind::Unavailable,
                    io::Error::other("test storage outage"),
                )),
                EventSpineOutcome::Failed,
            ),
            (
                Err(InboxStoreError::with_source(
                    InboxStoreErrorKind::Invariant,
                    io::Error::other("test invariant failure"),
                )),
                EventSpineOutcome::Failed,
            ),
        ];
        for (result, expected) in cases {
            assert_eq!(quarantine_outcome(&result), expected);
        }
    }

    #[test]
    fn settlement_result_determines_confirmed_telemetry_outcome() -> Result<(), ConsumeError> {
        let retry = DeliveryDisposition::RetryAfter(RetryDelay::new(Duration::from_millis(1))?);
        for (disposition, expected) in [
            (
                DeliveryDisposition::Acknowledge,
                EventSpineOutcome::Succeeded,
            ),
            (retry, EventSpineOutcome::RetryScheduled),
            (
                DeliveryDisposition::Quarantined,
                EventSpineOutcome::Quarantined,
            ),
        ] {
            assert_eq!(settlement_outcome(disposition, &Ok(())), expected);
            assert_eq!(
                settlement_outcome(
                    disposition,
                    &Err(ConsumeError::unavailable("confirmation lost"))
                ),
                EventSpineOutcome::Failed
            );
        }
        Ok(())
    }
}
