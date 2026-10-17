use std::collections::BTreeSet;

use edgeagent_contracts::{Component, MessageKind};

use super::{Binding, StreamPlan};

/// Credential-free NATS allowlists; absent subjects remain denied by the server.
///
/// Install on a distinct user with no inherited/default grants. Callers must
/// configure the SDK's custom inbox prefix to [`Self::inbox_prefix`]. This is
/// subject authorization, not payload/source authentication or credential setup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubjectPermissions {
    component: Option<Component>,
    inbox_prefix: String,
    publish: BTreeSet<String>,
    subscribe: BTreeSet<String>,
}

impl SubjectPermissions {
    fn new(component: Option<Component>, namespace: &str, role: &str) -> Self {
        let inbox_prefix = format!("_INBOX.{namespace}.{role}");
        Self {
            component,
            subscribe: BTreeSet::from([format!("{inbox_prefix}.>")]),
            inbox_prefix,
            publish: BTreeSet::new(),
        }
    }

    /// Runtime component, or `None` for the distinct provisioning principal.
    #[must_use]
    pub const fn component(&self) -> Option<Component> {
        self.component
    }
    /// Required SDK `custom_inbox_prefix`, unique to this role/environment.
    #[must_use]
    pub fn inbox_prefix(&self) -> &str {
        &self.inbox_prefix
    }
    /// Exact publish subjects plus scoped acknowledgement wildcards.
    #[must_use]
    pub const fn publish_allow(&self) -> &BTreeSet<String> {
        &self.publish
    }
    /// Subscribe allowlist: this principal's private reply inbox only.
    #[must_use]
    pub const fn subscribe_allow(&self) -> &BTreeSet<String> {
        &self.subscribe
    }
    /// Render a credential-free `permissions` object for a NATS user definition.
    ///
    /// All interpolated subjects originate in validated registry tokens or fixed
    /// API templates. No credential, arbitrary subject, or user text is accepted.
    #[must_use]
    pub fn nats_config(&self) -> String {
        fn list(values: &BTreeSet<String>) -> String {
            values
                .iter()
                .map(|value| format!("{value:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
        format!(
            "{{ publish: {{ allow: [{}] }}, subscribe: {{ allow: [{}] }} }}",
            list(&self.publish),
            list(&self.subscribe)
        )
    }
}

pub(super) fn derive(
    streams: &[StreamPlan],
    publishers: &[Binding],
    namespace: &str,
) -> (Vec<SubjectPermissions>, SubjectPermissions) {
    let mut runtime = Vec::new();
    let mut provisioner = SubjectPermissions::new(None, namespace, "provisioner");
    for stream in streams {
        let name = &stream.config.name;
        // Every private StreamPlan is constructed with exactly one subject.
        let subject = &stream.config.subjects[0];
        provisioner.publish.extend([
            format!("$JS.API.STREAM.INFO.{name}"),
            format!("$JS.API.STREAM.CREATE.{name}"),
            format!("$JS.API.CONSUMER.NAMES.{name}"),
        ]);
        if stream.definition.kind() == MessageKind::Event {
            role(&mut runtime, stream.definition.owner(), namespace)
                .publish
                .insert(subject.clone());
        } else {
            for binding in publishers
                .iter()
                .filter(|binding| binding.message_type == stream.definition.message_type())
            {
                role(&mut runtime, binding.component, namespace)
                    .publish
                    .insert(subject.clone());
            }
        }
        for consumer in &stream.consumers {
            // Plans always set the name. Derive it from the same component token
            // rather than unwrapping SDK optional fields.
            let durable = consumer.component.as_str().replace('.', "_");
            provisioner.publish.extend([
                format!("$JS.API.CONSUMER.INFO.{name}.{durable}"),
                format!("$JS.API.CONSUMER.CREATE.{name}.{durable}.{subject}"),
            ]);
            role(&mut runtime, consumer.component, namespace)
                .publish
                .extend([
                    format!("$JS.API.STREAM.INFO.{name}"),
                    format!("$JS.API.CONSUMER.INFO.{name}.{durable}"),
                    format!("$JS.API.CONSUMER.MSG.NEXT.{name}.{durable}"),
                    format!("$JS.ACK.{name}.{durable}.>"),
                ]);
        }
    }
    runtime.sort_by_key(|role| role.component.map(Component::as_str));
    (runtime, provisioner)
}

fn role<'a>(
    roles: &'a mut Vec<SubjectPermissions>,
    component: Component,
    namespace: &str,
) -> &'a mut SubjectPermissions {
    let index = match roles
        .iter()
        .position(|role| role.component == Some(component))
    {
        Some(index) => index,
        None => {
            roles.push(SubjectPermissions::new(
                Some(component),
                namespace,
                &component.as_str().replace('.', "_"),
            ));
            roles.len() - 1
        }
    };
    &mut roles[index]
}
