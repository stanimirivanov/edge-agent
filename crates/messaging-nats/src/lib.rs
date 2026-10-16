//! NATS JetStream implementation of application-owned messaging ports.
//!
//! Publication and intake own separate private modules. Broker metadata and
//! settlement stay behind the portable ports; provisioning belongs to callers.

#![forbid(unsafe_code)]

mod consumer;
mod publisher;
pub mod topology;

pub use consumer::JetStreamConsumer;
pub use publisher::JetStreamPublisher;
