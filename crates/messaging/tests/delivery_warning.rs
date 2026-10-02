//! Isolate the tracing callsite cache from parallel unit tests.

use std::sync::{Arc, Mutex};

use edgeagent_messaging::{
    ConsumeError, DeliveryAttempt, DeliveryDisposition, DeliveryMessageKey, DeliveryMetadata,
    DeliverySettlement, DeliverySubject, MessageDelivery, SettlementFuture,
};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

struct WarningSubscriber(Arc<Mutex<Vec<String>>>);

impl Subscriber for WarningSubscriber {
    fn register_callsite(
        &self,
        _metadata: &'static Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::WARN
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = WarningFields::default();
        event.record(&mut fields);
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(fields.0);
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[derive(Default)]
struct WarningFields(String);

impl Visit for WarningFields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(field.name());
        self.0.push('=');
        self.0.push_str(&format!("{value:?}"));
        self.0.push(';');
    }
}

struct NoopSettlement;

impl DeliverySettlement for NoopSettlement {
    fn settle(self: Box<Self>, _disposition: DeliveryDisposition) -> SettlementFuture {
        Box::pin(async { Ok(()) })
    }
}

#[test]
fn unsettled_drop_warns_once_without_payload_and_settle_does_not_warn() -> Result<(), ConsumeError>
{
    let records = Arc::new(Mutex::new(Vec::new()));
    let payload = b"private-unsettled-sentinel-7391";
    let metadata = || -> Result<DeliveryMetadata, ConsumeError> {
        Ok(DeliveryMetadata::new(
            DeliveryMessageKey::new("orders:11")?,
            DeliverySubject::new("events.subject")?,
            DeliveryAttempt::new(1)?,
        ))
    };
    tracing::subscriber::with_default(WarningSubscriber(Arc::clone(&records)), || {
        drop(MessageDelivery::new(
            payload.to_vec(),
            metadata()?,
            Box::new(NoopSettlement),
        ));
        drop(
            MessageDelivery::new(payload.to_vec(), metadata()?, Box::new(NoopSettlement))
                .settle(DeliveryDisposition::Acknowledge),
        );
        Ok::<(), ConsumeError>(())
    })?;

    let records = records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(records.len(), 1);
    assert!(records[0].contains("delivery_attempt"));
    assert!(records[0].contains("payload_bytes"));
    assert!(!records[0].contains("private-unsettled-sentinel-7391"));
    assert!(!records[0].contains(&format!("{payload:?}")));
    Ok(())
}
