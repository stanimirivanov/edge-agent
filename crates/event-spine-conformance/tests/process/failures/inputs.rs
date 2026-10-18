use super::*;
use edgeagent_messaging::{MessagePublisher, PublishDisposition};
use edgeagent_messaging_nats::JetStreamPublisher;

async fn inspector(url: &str) -> Result<async_nats::jetstream::Context, TestError> {
    Ok(async_nats::jetstream::new(
        broker::connect(url, "inspector", "_INBOX.inspector").await?,
    ))
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL and owned pinned broker; use cargo xtask m02-qualification"]
async fn duplicate_after_broker_window_preserves_one_committed_transition() -> TestResult {
    broker::with_broker(&plan()?, |url| async move {
        prepare(&url).await?;
        let context = inspector(&url).await?;
        let topology = plan()?;
        let stream = topology
            .streams()
            .iter()
            .find(|s| s.definition() == definitions()[0])
            .ok_or("command stream absent")?;
        let mut config = stream.config().clone();
        config.duplicate_window = Duration::from_secs(2);
        context.update_stream(config).await?;
        database::with_database(|db| {
            Box::pin(async move {
                accepted(db, &url).await?;
                let bytes = outbox(db).await?.2;
                let envelope = edgeagent_contracts::MessageEnvelope::from_json(&bytes)?;
                let publisher = JetStreamPublisher::from_context(context);
                assert_eq!(
                    publisher
                        .publish(definitions()[0], &envelope)
                        .await?
                        .disposition(),
                    PublishDisposition::Duplicate
                );
                assert_eq!(db.worker("consumer", true, &url, true).await?, "applied");
                // Poll an actual persisted-vs-duplicate receipt, not a guessed sleep.
                // Every retry keeps the same immutable identity/content.
                tokio::time::timeout(Duration::from_secs(5), async {
                    let mut poll = tokio::time::interval(Duration::from_millis(25));
                    loop {
                        poll.tick().await;
                        if publisher
                            .publish(definitions()[0], &envelope)
                            .await?
                            .disposition()
                            == PublishDisposition::Persisted
                        {
                            return Ok::<(), TestError>(());
                        }
                    }
                })
                .await??;
                assert_eq!(db.worker("consumer", true, &url, true).await?, "duplicate");
                assert_counts(db).await?;
                pending(&url, 0).await
            })
        })
        .await
    })
    .await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL and owned pinned broker; use cargo xtask m02-qualification"]
async fn invalid_inputs_quarantine_or_halt_without_false_acknowledgement() -> TestResult {
    broker::with_broker(&plan()?, |url| async move {
        prepare(&url).await?;
        database::with_database(|db| {
            Box::pin(async move {
                assert_eq!(db.worker("producer", false, &url, true).await?, "accepted");
                let original = outbox(db).await?.2;
                let value: serde_json::Value = serde_json::from_slice(&original)?;
                let mut unsupported = value.clone();
                unsupported["type"] = "com.edgeagent.conformance.unknown.v1".into();
                let mut poison = value;
                poison["data"]["value"] = (-1).into();
                let context = inspector(&url).await?;
                for (bytes, expected) in [
                    (
                        b"{private-invalid-fixture".to_vec(),
                        "quarantined:envelope_invalid",
                    ),
                    (
                        serde_json::to_vec(&unsupported)?,
                        "quarantined:routing_invalid",
                    ),
                    (serde_json::to_vec(&poison)?, "quarantined:invalid_probe"),
                ] {
                    context
                        .publish(definitions()[0].subject()?, bytes.clone().into())
                        .await?
                        .await?;
                    assert_eq!(db.worker("consumer", true, &url, true).await?, expected);
                    assert!(
                        scenarios::quarantine(db)
                            .await?
                            .iter()
                            .any(|(_, _, retained)| retained == &bytes)
                    );
                    pending(&url, 0).await?;
                }
                assert_eq!(db.count(true, "edgeagent_message_quarantine").await?, 3);
                for table in [
                    "edgeagent_message_inbox",
                    "conformance_probe",
                    "conformance_command",
                    "edgeagent_message_outbox",
                ] {
                    assert_eq!(db.count(true, table).await?, 0);
                }
                // Broker's explicit size rejection proves non-acceptance, unlike a
                // timeout. It is deliberately separate from adapter intake bounds.
                let rejected = context
                    .publish(
                        definitions()[0].subject()?,
                        vec![b'x'; edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES + 1025].into(),
                    )
                    .await?
                    .await;
                let rejected = rejected.err().ok_or("broker size rejection absent")?;
                let cause = std::error::Error::source(&rejected)
                    .and_then(|source| source.downcast_ref::<async_nats::jetstream::Error>())
                    .ok_or("typed broker size rejection absent")?;
                assert_eq!(
                    cause.error_code(),
                    async_nats::jetstream::ErrorCode::STREAM_MESSAGE_EXCEEDS_MAXIMUM
                );
                assert_eq!(scenarios::durable(&url).await?.info().await?.num_pending, 0);
                // Broker header reserve permits this body, but the portable adapter
                // must halt without fabricating a delivery or quarantine/ACK.
                context
                    .publish(
                        definitions()[0].subject()?,
                        vec![b'x'; edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES + 1].into(),
                    )
                    .await?
                    .await?;
                assert_eq!(
                    db.worker("consumer", true, &url, true).await?,
                    "protocol_halt"
                );
                pending(&url, 1).await?;
                assert_eq!(db.count(true, "edgeagent_message_quarantine").await?, 3);
                Ok(())
            })
        })
        .await
    })
    .await
}
