use async_nats::jetstream::{
    Context, ErrorCode,
    consumer::{self, IntoConsumerConfig},
    context::{ConsumerInfoErrorKind, GetStreamErrorKind},
    stream::{self, Stream},
};
use futures_util::TryStreamExt;

use super::{StreamPlan, TopologyError, TopologyErrorKind, TopologyPlan};

/// Inspect all existing resources, then create missing streams and consumers.
///
/// This is an explicit control-plane action, never part of adapter startup.
/// Install the plan's provisioning ACL and use a quiesced single writer. Requests
/// use the caller's configured SDK timeout. No update, purge, delete, rollback,
/// credential management, or cross-resource atomicity is performed. After an
/// error/cancellation, inspect and retry the same plan: completed creates remain.
///
/// # Errors
/// Returns `Conflict` for incompatible existing resources (including undeclared
/// consumers), or `Unavailable` retaining the actual SDK cause on I/O failure.
/// A timeout/cancellation cannot establish whether a create committed.
pub async fn provision(context: &Context, plan: &TopologyPlan) -> Result<(), TopologyError> {
    // Complete the read-only preflight before issuing any write. It is not a
    // distributed lock; a concurrent operator must not mutate active topology.
    for expected in &plan.streams {
        if let Some(stream) = existing(context, &expected.config.name).await? {
            inspect(&stream, expected).await?;
        }
    }
    for expected in &plan.streams {
        let stream = match existing(context, &expected.config.name).await? {
            Some(stream) => stream,
            None => context
                .create_stream(expected.config.clone())
                .await
                .map_err(TopologyError::io)?,
        };
        inspect(&stream, expected).await?;
        for durable in &expected.consumers {
            let name = durable.component.as_str().replace('.', "_");
            match stream.consumer_info(&name).await {
                Ok(info) => {
                    verify_consumer(&info.config, &durable.config.clone().into_consumer_config())?
                }
                Err(error) if error.kind() == ConsumerInfoErrorKind::NotFound => {
                    let created = stream
                        .create_consumer_strict(durable.config.clone())
                        .await
                        .map_err(TopologyError::io)?;
                    verify_consumer(
                        &created.cached_info().config,
                        &durable.config.clone().into_consumer_config(),
                    )?;
                }
                Err(error) => return Err(TopologyError::io(error)),
            }
        }
    }
    Ok(())
}

async fn existing(context: &Context, name: &str) -> Result<Option<Stream>, TopologyError> {
    match context.get_stream(name).await {
        Ok(stream) => Ok(Some(stream)),
        Err(error) if matches!(error.kind(), GetStreamErrorKind::JetStream(ref error) if error.error_code() == ErrorCode::STREAM_NOT_FOUND) => {
            Ok(None)
        }
        Err(error) => Err(TopologyError::io(error)),
    }
}

async fn inspect(stream: &Stream, expected: &StreamPlan) -> Result<(), TopologyError> {
    verify_stream(&stream.cached_info().config, &expected.config)?;
    let mut names = stream.consumer_names();
    while let Some(name) = names.try_next().await.map_err(TopologyError::io)? {
        let durable = expected
            .consumers
            .iter()
            .find(|durable| durable.config.name.as_deref() == Some(name.as_str()))
            .ok_or_else(conflict)?;
        let info = stream
            .consumer_info(name)
            .await
            .map_err(TopologyError::io)?;
        verify_consumer(&info.config, &durable.config.clone().into_consumer_config())?;
    }
    Ok(())
}

// Compare the full pinned SDK configuration, including disabled capabilities.
// Ignore only the server's three informational version/feature metadata keys,
// not arbitrary metadata or behavior-changing fields. Explicit defaults in the
// plan avoid interpreting an unlimited broker default as a configured bound.
pub(super) fn verify_stream(
    actual: &stream::Config,
    expected: &stream::Config,
) -> Result<(), TopologyError> {
    let mut actual = actual.clone();
    server_metadata(&mut actual.metadata);
    // The server omits the default persistence mode in its response. An absent
    // value means Default, never Async; preserve exact comparison otherwise.
    if actual.persist_mode.is_none() {
        actual.persist_mode = Some(stream::PersistenceMode::Default);
    }
    if actual == *expected {
        Ok(())
    } else {
        Err(conflict())
    }
}

pub(super) fn verify_consumer(
    actual: &consumer::Config,
    expected: &consumer::Config,
) -> Result<(), TopologyError> {
    let mut actual = actual.clone();
    server_metadata(&mut actual.metadata);
    if actual == *expected {
        Ok(())
    } else {
        Err(conflict())
    }
}

fn server_metadata(metadata: &mut std::collections::HashMap<String, String>) {
    for key in ["_nats.level", "_nats.req.level", "_nats.ver"] {
        metadata.remove(key);
    }
}

fn conflict() -> TopologyError {
    TopologyError::new(TopologyErrorKind::Conflict)
}
