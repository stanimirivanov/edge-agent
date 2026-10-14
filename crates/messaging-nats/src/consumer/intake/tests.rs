use super::*;
use edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES;
use edgeagent_messaging::{
    DeliveryAttempt, DeliveryDisposition, DeliveryMessageKey, DeliveryMetadata, DeliverySettlement,
    DeliverySubject, MessageDelivery, SettlementFuture,
};
use std::error::Error;

#[test]
fn malformed_metadata_halts_this_consumer_without_retrying_the_pull() {
    let (mut gate, drops) = intake_probe();
    assert!(gate.active().is_ok());
    assert!(matches!(gate.validate(Ok(41_u64)), Ok(41)));
    assert!(gate.active().is_ok());

    let error = ConsumeError::protocol("invalid broker metadata");
    assert_eq!(
        gate.validate::<u64>(Err(error))
            .err()
            .map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );
    assert_eq!(
        gate.active().err().map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
    drop(gate);
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
}

struct IntakeProbe(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Drop for IntakeProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl futures_util::Stream for IntakeProbe {
    type Item = ();

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Pending
    }
}

fn intake_probe() -> (
    ConsumerIntake<IntakeProbe>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    (ConsumerIntake::Active(IntakeProbe(drops.clone())), drops)
}

#[test]
fn terminal_pull_results_release_the_stream_and_repeated_receive_stays_closed()
-> Result<(), Box<dyn Error>> {
    use async_nats::jetstream::consumer::pull::{MessagesError, MessagesErrorKind};
    for result in [
        None,
        Some(Err(MessagesError::new(MessagesErrorKind::ConsumerDeleted))),
        Some(Err(MessagesError::new(
            MessagesErrorKind::PushBasedConsumer,
        ))),
    ] {
        let expected = result
            .as_ref()
            .and_then(|result: &Result<(), MessagesError>| {
                result.as_ref().err().map(MessagesError::kind)
            });
        let (mut intake, drops) = intake_probe();
        let error = intake.resolve_pull(result).err().ok_or("terminal error")?;
        assert_eq!(error.kind(), ConsumeErrorKind::Unavailable);
        assert_eq!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<MessagesError>())
                .map(MessagesError::kind),
            expected
        );
        for _ in 0..2 {
            assert_eq!(
                intake.active().err().map(|error| error.kind()),
                Some(ConsumeErrorKind::Unavailable)
            );
        }
        assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
        drop(intake);
        assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
    Ok(())
}

#[test]
fn transient_pull_errors_preserve_stream_and_typed_cause() -> Result<(), Box<dyn Error>> {
    use async_nats::jetstream::consumer::pull::{MessagesError, MessagesErrorKind};
    for kind in [
        MessagesErrorKind::MissingHeartbeat,
        MessagesErrorKind::Pull,
        MessagesErrorKind::NoResponders,
        MessagesErrorKind::Other,
    ] {
        let (mut intake, drops) = intake_probe();
        let error = intake
            .resolve_pull::<()>(Some(Err(MessagesError::with_source(
                kind,
                std::io::Error::other("private-pull-sentinel"),
            ))))
            .err()
            .ok_or("transient error")?;
        assert_eq!(error.kind(), ConsumeErrorKind::Unavailable);
        let cause = error
            .source()
            .and_then(|source| source.downcast_ref::<MessagesError>())
            .ok_or("SDK cause")?;
        assert_eq!(cause.kind(), kind);
        assert!(
            cause
                .source()
                .is_some_and(|source| source.is::<std::io::Error>())
        );
        assert!(!error.to_string().contains("private-pull-sentinel"));
        assert!(!format!("{error:?}").contains("private-pull-sentinel"));
        assert!(intake.active().is_ok());
        assert_eq!(intake.resolve_pull(Some(Ok(42)))?, 42);
        assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
    Ok(())
}

struct NoopSettlement;

impl DeliverySettlement for NoopSettlement {
    fn settle(self: Box<Self>, _disposition: DeliveryDisposition) -> SettlementFuture {
        Box::pin(async { Ok(()) })
    }
}

#[test]
fn oversized_payload_halts_this_consumer_before_another_pull() -> Result<(), Box<dyn Error>> {
    let metadata = DeliveryMetadata::new(
        DeliveryMessageKey::new("6:STREAM:1")?,
        DeliverySubject::new("edgeagent.command.execution.unit-intake.v1")?,
        DeliveryAttempt::new(1)?,
    );
    let (mut gate, drops) = intake_probe();

    let result = gate.validate(MessageDelivery::new(
        vec![0; MAX_PORTABLE_MESSAGE_BYTES + 1],
        metadata,
        Box::new(NoopSettlement),
    ));

    assert_eq!(
        result.err().map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );
    assert_eq!(
        gate.active().err().map(|error| error.kind()),
        Some(ConsumeErrorKind::Protocol)
    );
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn cancelling_receive_releases_its_worker_without_closing_intake() -> Result<(), Box<dyn Error>> {
    use futures_util::FutureExt;
    let (intake, drops) = intake_probe();
    let ConsumerIntake::Active(stream) = intake else {
        return Err("active probe".into());
    };
    let mut configured = ConsumerIntake::Active(());
    assert!(receive_from(stream).now_or_never().is_none());
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert!(configured.active().is_ok());
    Ok(())
}

#[test]
fn intake_debug_redacts_its_resource_in_every_state() {
    let private = b"private-consumer-sentinel-713";
    for intake in [
        ConsumerIntake::Active(private.to_vec()),
        ConsumerIntake::Halted,
        ConsumerIntake::Closed,
    ] {
        let debug = format!("{intake:?}");
        assert!(!debug.contains("private-consumer-sentinel-713"));
        assert!(!debug.contains(&format!("{private:?}")));
        assert!(matches!(debug.as_str(), "Active" | "Halted" | "Closed"));
    }
}

#[test]
fn completed_receive_releases_its_worker_before_handoff() {
    use futures_util::{FutureExt, StreamExt};
    let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let probe = IntakeProbe(drops.clone());
    let stream = futures_util::stream::iter([42]).inspect(move |_| {
        let _keep_owned_until_stream_drop = &probe;
    });
    assert_eq!(receive_from(stream).now_or_never(), Some(Some(42)));
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
}
