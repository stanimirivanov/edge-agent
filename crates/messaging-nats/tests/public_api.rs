//! Offline contracts for the stable facade and adapter auto traits.

use edgeagent_messaging::{MessageConsumer, MessagePublisher};
use edgeagent_messaging_nats::{JetStreamConsumer, JetStreamPublisher};
use std::fmt::Debug;

#[test]
fn crate_root_exports_preserve_port_and_auto_trait_contracts() {
    fn publisher<T: MessagePublisher + Clone + Debug + Send + Sync>() {}
    fn consumer<T: MessageConsumer + Debug + Send>() {}
    publisher::<JetStreamPublisher>();
    consumer::<JetStreamConsumer>();
    let _: fn(async_nats::Client) -> JetStreamPublisher = JetStreamPublisher::new;
    let _: fn(async_nats::jetstream::Context) -> JetStreamPublisher =
        JetStreamPublisher::from_context;
}
