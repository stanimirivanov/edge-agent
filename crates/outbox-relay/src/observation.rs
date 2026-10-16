//! Explicit, result-driven recording; no diagnostics or recovery occur in Drop.

use edgeagent_contracts::MessageEnvelope;
use edgeagent_messaging::{OutboxStoreError, PublishDisposition, PublishError, PublishReceipt};
use edgeagent_telemetry::{
    EventSpineContext, EventSpineOutcome, EventSpineStage, record_event_spine_operation,
};
use std::time::Instant;

pub(super) struct StageStart(Instant);

/// Own only approved correlation fields, never raw persisted columns or payloads.
pub(super) struct RelayObservation {
    context: EventSpineContext,
    attempt: u32,
}

impl RelayObservation {
    pub(super) fn rejected(attempt: u32) -> Self {
        Self {
            context: EventSpineContext::without_identity(),
            attempt,
        }
    }

    pub(super) fn validated(envelope: &MessageEnvelope, attempt: u32) -> Self {
        Self {
            context: EventSpineContext::from_envelope(envelope),
            attempt,
        }
    }

    pub(super) fn start_stage(&self) -> StageStart {
        StageStart(Instant::now())
    }

    pub(super) fn publication(
        &self,
        started: StageStart,
        result: &Result<PublishReceipt, PublishError>,
    ) {
        let outcome = match result {
            Ok(receipt) => match receipt.disposition() {
                PublishDisposition::Persisted => EventSpineOutcome::Succeeded,
                PublishDisposition::Duplicate => EventSpineOutcome::Duplicate,
            },
            Err(_) => EventSpineOutcome::Failed,
        };
        self.record(started, EventSpineStage::Publication, outcome);
    }

    pub(super) fn persistence(
        &self,
        started: StageStart,
        result: &Result<(), OutboxStoreError>,
        committed_outcome: EventSpineOutcome,
    ) {
        self.record(
            started,
            EventSpineStage::Persistence,
            if result.is_ok() {
                committed_outcome
            } else {
                EventSpineOutcome::Failed
            },
        );
    }

    fn record(&self, started: StageStart, stage: EventSpineStage, outcome: EventSpineOutcome) {
        record_event_spine_operation(
            &self.context,
            stage,
            outcome,
            self.attempt,
            started.0.elapsed(),
        );
    }
}
