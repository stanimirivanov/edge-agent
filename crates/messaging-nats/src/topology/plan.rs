use std::time::Duration;

use async_nats::jetstream::{
    consumer::{self, pull},
    stream,
};
use edgeagent_contracts::{
    Component, MAX_PORTABLE_MESSAGE_BYTES, MessageDefinition, MessageKind, MessageRegistry,
};

use super::{SubjectPermissions, TopologyError, TopologyErrorKind};

/// An explicit command-publisher or event-subscriber grant.
///
/// Construction records intent only; [`TopologyPlan::new`] validates the entire
/// binding set against the registry before deriving any resources or permissions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Binding {
    pub(super) component: Component,
    pub(super) message_type: &'static str,
}

impl Binding {
    /// Record the component and exact versioned registry type.
    #[must_use]
    pub const fn new(component: Component, message_type: &'static str) -> Self {
        Self {
            component,
            message_type,
        }
    }
}

/// Validated per-stream capacity and per-consumer admission bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TopologyLimits {
    max_messages: i64,
    max_bytes: i64,
    max_ack_pending: i64,
    max_waiting: i64,
}

impl TopologyLimits {
    /// Bound messages (1..=1,000,000), bytes (257 KiB..=1 TiB), outstanding
    /// acknowledgements (1..=1024), and waiting pulls (1..=1024).
    ///
    /// # Errors
    /// Rejects nonpositive, unbounded, or unsupported values before I/O.
    pub fn new(
        max_messages: i64,
        max_bytes: i64,
        max_ack_pending: i64,
        max_waiting: i64,
    ) -> Result<Self, TopologyError> {
        let minimum_bytes =
            i64::try_from(MAX_PORTABLE_MESSAGE_BYTES + super::BROKER_HEADER_RESERVE_BYTES)
                .map_err(|_| invalid())?;
        if !(1..=1_000_000).contains(&max_messages)
            || !(minimum_bytes..=1_099_511_627_776).contains(&max_bytes)
            || !(1..=1024).contains(&max_ack_pending)
            || !(1..=1024).contains(&max_waiting)
        {
            return Err(invalid());
        }
        Ok(Self {
            max_messages,
            max_bytes,
            max_ack_pending,
            max_waiting,
        })
    }
}

impl Default for TopologyLimits {
    fn default() -> Self {
        Self {
            max_messages: 10_000,
            max_bytes: 64 * 1024 * 1024,
            max_ack_pending: 32,
            max_waiting: 16,
        }
    }
}

/// One exact-filter durable pull consumer; replicas share its stable name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumerPlan {
    pub(super) component: Component,
    pub(super) config: pull::Config,
}

impl ConsumerPlan {
    /// Return the authorized handler or independent event subscriber.
    #[must_use]
    pub const fn component(&self) -> Component {
        self.component
    }
    /// Inspect the immutable expected broker configuration.
    #[must_use]
    pub const fn config(&self) -> &pull::Config {
        &self.config
    }
}

/// A file-backed exact-subject stream and its declared durable consumers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamPlan {
    pub(super) definition: MessageDefinition,
    pub(super) config: stream::Config,
    pub(super) consumers: Vec<ConsumerPlan>,
}

impl StreamPlan {
    /// Return the registry definition that owns this stream's meaning.
    #[must_use]
    pub const fn definition(&self) -> MessageDefinition {
        self.definition
    }
    /// Inspect the immutable expected broker configuration.
    #[must_use]
    pub const fn config(&self) -> &stream::Config {
        &self.config
    }
    /// Return the owning command durable or independent event durables.
    #[must_use]
    pub fn consumers(&self) -> &[ConsumerPlan] {
        &self.consumers
    }
}

/// Deterministic single-node broker topology; constructing it performs no I/O.
///
/// Exact subjects have separate streams so retention classes cannot bleed into
/// each other. Environment credentials, replicas/HA qualification, and installing
/// the generated ACLs remain explicit deployment responsibilities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TopologyPlan {
    pub(super) streams: Vec<StreamPlan>,
    pub(super) runtime: Vec<SubjectPermissions>,
    pub(super) provisioner: SubjectPermissions,
}

impl TopologyPlan {
    /// Derive resources and grants from a validated registry and explicit bindings.
    ///
    /// `namespace` is a 1..=32 byte lowercase token, used only for resource names
    /// and private reply inboxes, never to rewrite published message subjects.
    /// Command handlers/event producers come from the registry. Only command
    /// publisher and event subscriber grants may be supplied. Registry/resource
    /// and binding counts are bounded at 128 and 640 respectively.
    ///
    /// # Errors
    /// Rejects empty/oversized registries, invalid names, duplicate/unknown/wrong-
    /// kind bindings, and resource names exceeding the supported 200-byte bound.
    pub fn new(
        registry: MessageRegistry<'_>,
        namespace: &str,
        command_publishers: &[Binding],
        event_subscribers: &[Binding],
        limits: TopologyLimits,
    ) -> Result<Self, TopologyError> {
        if namespace.is_empty()
            || namespace.len() > 32
            || !namespace
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || registry.definitions().is_empty()
            || registry.definitions().len() > 128
        {
            return Err(invalid());
        }
        validate_bindings(registry, command_publishers, MessageKind::Command)?;
        validate_bindings(registry, event_subscribers, MessageKind::Event)?;
        let mut streams = Vec::new();
        for definition in registry.definitions() {
            let subject = definition.subject().map_err(|_| invalid())?;
            // Registry tokens exclude underscores, so replacing dots is injective.
            let name = format!("{namespace}_{}", subject.replace('.', "_"));
            if name.len() > 200 {
                return Err(invalid());
            }
            let mut consumers = Vec::new();
            if definition.kind() == MessageKind::Command {
                consumers.push(consumer(definition.owner(), &subject, limits));
            } else {
                for binding in event_subscribers
                    .iter()
                    .filter(|binding| binding.message_type == definition.message_type())
                {
                    consumers.push(consumer(binding.component, &subject, limits));
                }
            }
            consumers.sort_by_key(|consumer| consumer.component.as_str());
            streams.push(StreamPlan {
                definition: *definition,
                config: stream::Config {
                    name,
                    subjects: vec![subject],
                    storage: stream::StorageType::File,
                    retention: if definition.kind() == MessageKind::Command {
                        stream::RetentionPolicy::WorkQueue
                    } else {
                        stream::RetentionPolicy::Limits
                    },
                    max_age: Duration::from_secs(definition.retention().seconds()),
                    max_messages: limits.max_messages,
                    max_bytes: limits.max_bytes,
                    max_messages_per_subject: -1,
                    max_consumers: if definition.kind() == MessageKind::Command {
                        1
                    } else {
                        5
                    },
                    max_message_size: i32::try_from(
                        MAX_PORTABLE_MESSAGE_BYTES + super::BROKER_HEADER_RESERVE_BYTES,
                    )
                    .map_err(|_| invalid())?,
                    discard: stream::DiscardPolicy::New,
                    num_replicas: 1,
                    duplicate_window: Duration::from_secs(120),
                    deny_delete: true,
                    deny_purge: true,
                    persist_mode: Some(stream::PersistenceMode::Default),
                    compression: Some(stream::Compression::None),
                    ..stream::Config::default()
                },
                consumers,
            });
        }
        streams.sort_by(|a, b| a.config.name.cmp(&b.config.name));
        let (runtime, provisioner) =
            super::permissions::derive(&streams, command_publishers, namespace);
        Ok(Self {
            streams,
            runtime,
            provisioner,
        })
    }

    /// Return every expected stream in deterministic name order.
    #[must_use]
    pub fn streams(&self) -> &[StreamPlan] {
        &self.streams
    }
    /// Return least-privilege runtime roles in deterministic component order.
    #[must_use]
    pub fn runtime_permissions(&self) -> &[SubjectPermissions] {
        &self.runtime
    }
    /// Return create/read-only control-plane grants, without data-plane access.
    #[must_use]
    pub const fn provisioner_permissions(&self) -> &SubjectPermissions {
        &self.provisioner
    }
}

fn consumer(component: Component, subject: &str, limits: TopologyLimits) -> ConsumerPlan {
    let name = component.as_str().replace('.', "_");
    ConsumerPlan {
        component,
        config: pull::Config {
            durable_name: Some(name.clone()),
            name: Some(name),
            filter_subject: subject.into(),
            ack_policy: consumer::AckPolicy::Explicit,
            ack_wait: Duration::from_secs(120),
            max_deliver: -1,
            max_ack_pending: limits.max_ack_pending,
            max_waiting: limits.max_waiting,
            max_batch: 1,
            max_expires: Duration::from_secs(30),
            num_replicas: 1,
            ..pull::Config::default()
        },
    }
}

fn validate_bindings(
    registry: MessageRegistry<'_>,
    bindings: &[Binding],
    kind: MessageKind,
) -> Result<(), TopologyError> {
    if bindings.len() > 640 {
        return Err(invalid());
    }
    for (index, binding) in bindings.iter().enumerate() {
        if bindings[..index].contains(binding)
            || registry
                .resolve(binding.message_type)
                .is_none_or(|definition| definition.kind() != kind)
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn invalid() -> TopologyError {
    TopologyError::new(TopologyErrorKind::InvalidInput)
}
