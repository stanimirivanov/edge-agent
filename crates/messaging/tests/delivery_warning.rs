//! Exercise the delivery lifecycle in one isolated tracing executable.

use std::collections::BTreeMap;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use bytes::Bytes;
use edgeagent_messaging::{
    ConsumeError, ConsumeErrorKind, DeliveryAttempt, DeliveryDisposition, DeliveryMessageKey,
    DeliveryMetadata, DeliverySettlement, DeliverySubject, MessageDelivery, RetryDelay,
    SettlementFuture,
};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

const PAYLOAD: &[u8] = b"private-unsettled-sentinel-7391";
const KEY: &str = "private.message:7391";
const SUBJECT: &str = "events.private-subject-7391";
const SOURCE: &str = "private-settlement-source-7391";

struct WarningSubscriber(Arc<Mutex<Vec<WarningFields>>>);

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
            .push(fields);
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[derive(Debug, Default)]
struct WarningFields(BTreeMap<String, String>);

impl Visit for WarningFields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

#[derive(Default)]
struct ProbeState {
    dispositions: Mutex<Vec<DeliveryDisposition>>,
    polls: AtomicUsize,
    drops: AtomicUsize,
}

enum Completion {
    Ready(Option<Result<(), ConsumeError>>),
    Pending,
}

struct ProbeSettlement {
    state: Arc<ProbeState>,
    completion: Completion,
}

impl DeliverySettlement for ProbeSettlement {
    fn settle(self: Box<Self>, disposition: DeliveryDisposition) -> SettlementFuture {
        self.state
            .dispositions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(disposition);
        let Self { state, completion } = *self;
        Box::pin(ProbeFuture { state, completion })
    }
}

struct ProbeFuture {
    state: Arc<ProbeState>,
    completion: Completion,
}

impl Future for ProbeFuture {
    type Output = Result<(), ConsumeError>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.state.polls.fetch_add(1, Ordering::SeqCst);
        match &mut this.completion {
            Completion::Ready(result) => Poll::Ready(result.take().unwrap_or_else(|| {
                Err(ConsumeError::protocol(
                    "settlement probe polled after completion",
                ))
            })),
            Completion::Pending => Poll::Pending,
        }
    }
}

impl Drop for ProbeFuture {
    fn drop(&mut self) {
        self.state.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct PayloadOwner(Arc<AtomicUsize>);

impl AsRef<[u8]> for PayloadOwner {
    fn as_ref(&self) -> &[u8] {
        PAYLOAD
    }
}

impl Drop for PayloadOwner {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn delivery(
    state: &Arc<ProbeState>,
    completion: Completion,
    payload: impl Into<Bytes>,
) -> Result<MessageDelivery, ConsumeError> {
    MessageDelivery::new(
        payload,
        DeliveryMetadata::new(
            DeliveryMessageKey::new(KEY)?,
            DeliverySubject::new(SUBJECT)?,
            DeliveryAttempt::new(7)?,
        ),
        Box::new(ProbeSettlement {
            state: Arc::clone(state),
            completion,
        }),
    )
}

fn assert_disposition(state: &ProbeState, disposition: DeliveryDisposition) {
    let calls = state
        .dispositions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(*calls, [disposition]);
}

fn assert_warnings(records: &Mutex<Vec<WarningFields>>, expected: usize) {
    let records = records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(records.len(), expected);
    for (index, WarningFields(fields)) in records.iter().enumerate() {
        assert_eq!(fields.get("delivery_attempt"), Some(&"7".into()));
        assert_eq!(
            fields.get("payload_bytes"),
            Some(&PAYLOAD.len().to_string())
        );
        if index == 0 {
            assert_eq!(fields.len(), 3);
            assert!(fields.get("message").is_some_and(|value| {
                value.contains("message delivery dropped without settlement")
            }));
        } else {
            assert_eq!(fields.len(), 4);
            assert!(fields.get("message").is_some_and(|value| {
                value.contains("message settlement dropped without completion")
            }));
            assert!(
                fields
                    .get("settlement_state")
                    .is_some_and(|value| value.contains("confirmation_unknown"))
            );
        }
        let rendered = format!("{fields:?}");
        for sentinel in [KEY, SUBJECT, SOURCE, "private-unsettled-sentinel-7391"] {
            assert!(!rendered.contains(sentinel));
        }
        assert!(!rendered.contains(&format!("{PAYLOAD:?}")));
    }
}

// Keep every tracing-sensitive case in this one test: thread-local subscribers
// cannot isolate the process-wide callsite cache from parallel test threads.
#[test]
fn delivery_lifecycle_warns_only_for_abandoned_work_and_forwards_once() -> Result<(), ConsumeError>
{
    let records = Arc::new(Mutex::new(Vec::new()));
    tracing::subscriber::with_default(WarningSubscriber(Arc::clone(&records)), || {
        let mut context = Context::from_waker(Waker::noop());
        let unsettled = Arc::new(ProbeState::default());
        drop(delivery(&unsettled, Completion::Pending, PAYLOAD)?);
        assert!(
            unsettled
                .dispositions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
        assert_eq!(unsettled.polls.load(Ordering::SeqCst), 0);
        assert_warnings(&records, 1);

        for disposition in [
            DeliveryDisposition::Acknowledge,
            DeliveryDisposition::RetryAfter(RetryDelay::new(Duration::from_millis(1))?),
            DeliveryDisposition::Quarantined,
        ] {
            let state = Arc::new(ProbeState::default());
            let mut future =
                delivery(&state, Completion::Ready(Some(Ok(()))), PAYLOAD)?.settle(disposition);
            assert_disposition(&state, disposition);
            assert_eq!(state.polls.load(Ordering::SeqCst), 0);
            assert!(matches!(
                future.as_mut().poll(&mut context),
                Poll::Ready(Ok(()))
            ));
            drop(future);
            assert_eq!(state.polls.load(Ordering::SeqCst), 1);
            assert_eq!(state.drops.load(Ordering::SeqCst), 1);
            assert_disposition(&state, disposition);
            assert_warnings(&records, 1);
        }

        let failed = Arc::new(ProbeState::default());
        let error =
            ConsumeError::with_source(ConsumeErrorKind::Unavailable, std::io::Error::other(SOURCE));
        let mut future = delivery(&failed, Completion::Ready(Some(Err(error))), PAYLOAD)?
            .settle(DeliveryDisposition::Acknowledge);
        let result = future.as_mut().poll(&mut context);
        assert!(matches!(&result, Poll::Ready(Err(_))));
        if let Poll::Ready(Err(error)) = result {
            assert_eq!(error.kind(), ConsumeErrorKind::Unavailable);
            assert_eq!(error.source().map(ToString::to_string), Some(SOURCE.into()));
            assert!(
                error
                    .source()
                    .is_some_and(|source| source.is::<std::io::Error>())
            );
        }
        drop(future);
        assert_disposition(&failed, DeliveryDisposition::Acknowledge);
        assert_eq!(failed.polls.load(Ordering::SeqCst), 1);
        assert_eq!(failed.drops.load(Ordering::SeqCst), 1);
        assert_warnings(&records, 1);

        let unpolled = Arc::new(ProbeState::default());
        let future = delivery(&unpolled, Completion::Pending, PAYLOAD)?
            .settle(DeliveryDisposition::Acknowledge);
        assert_disposition(&unpolled, DeliveryDisposition::Acknowledge);
        assert_eq!(unpolled.polls.load(Ordering::SeqCst), 0);
        drop(future);
        assert_eq!(unpolled.drops.load(Ordering::SeqCst), 1);
        assert_disposition(&unpolled, DeliveryDisposition::Acknowledge);
        assert_warnings(&records, 2);

        let pending = Arc::new(ProbeState::default());
        let payload_drops = Arc::new(AtomicUsize::new(0));
        let payload = Bytes::from_owner(PayloadOwner(Arc::clone(&payload_drops)));
        let delivery = delivery(&pending, Completion::Pending, payload)?;
        assert_eq!(payload_drops.load(Ordering::SeqCst), 0);
        let mut future = delivery.settle(DeliveryDisposition::Quarantined);
        assert_eq!(payload_drops.load(Ordering::SeqCst), 1);
        assert!(future.as_mut().poll(&mut context).is_pending());
        assert_eq!(pending.polls.load(Ordering::SeqCst), 1);
        assert_eq!(pending.drops.load(Ordering::SeqCst), 0);
        drop(future);
        assert_eq!(pending.drops.load(Ordering::SeqCst), 1);
        assert_disposition(&pending, DeliveryDisposition::Quarantined);
        assert_warnings(&records, 3);
        Ok(())
    })
}
