//! Finite control-plane composition. No network call may join an SQL transaction.

mod persistence;

use super::{IO_DEADLINE, WorkerError};
use async_nats::jetstream::{self, message::PublishMessage};
use edgeagent_contracts::Component;
use edgeagent_event_spine_conformance::replay::{
    ReplayCommand, ReplayPublisherScope, validate_inbound,
};
use edgeagent_messaging_nats::topology::TopologyPlan;
use sqlx::PgConnection;
use std::io::Write;
use tokio::io::AsyncReadExt;

pub(super) async fn authorize(
    database: &mut PgConnection,
    schema: &str,
    plan: &TopologyPlan,
    component: Component,
    bytes: &[u8],
    rollback: bool,
) -> Result<&'static str, WorkerError> {
    let actor = persistence::principal(database, schema, "o").await?;
    let request: ReplayCommand = serde_json::from_slice(bytes)?;
    persistence::authorize(database, plan, component, &actor, request, rollback).await
}

pub(super) async fn dispatch(
    database: &mut PgConnection,
    schema: &str,
    plan: &TopologyPlan,
    component: Component,
    context: jetstream::Context,
    pause: bool,
    input: &mut tokio::io::Stdin,
) -> Result<&'static str, WorkerError> {
    let actor = persistence::principal(database, schema, "d").await?;
    // Claim commits before any external call. Unknown results retain the lease;
    // a new generation may retry the same request after expiry, never from Drop.
    let consumer = plan
        .streams()
        .iter()
        .flat_map(|s| s.consumers())
        .find(|c| c.component() == component)
        .and_then(|c| c.config().durable_name.as_deref())
        .ok_or("replay consumer absent")?;
    let Some(mut claim) = persistence::claim(database, &actor, consumer).await? else {
        return Ok("idle");
    };
    validate_inbound(
        plan,
        component,
        &claim.consumer,
        &claim.subject,
        &claim.payload,
    )?;
    let scope = ReplayPublisherScope::new(plan, component)?;
    if !scope.subjects().contains(&claim.subject) {
        return Err("replay route denied".into());
    }
    let publish = PublishMessage::build()
        .payload(std::mem::take(&mut claim.payload).into())
        .message_id(format!("replay/{schema}/{}", claim.request));
    let receipt = tokio::time::timeout(IO_DEADLINE, async {
        context
            .send_publish(claim.subject.clone(), publish)
            .await?
            .await
    })
    .await??;
    let expected = plan
        .streams()
        .iter()
        .find(|s| s.config().subjects.contains(&claim.subject))
        .ok_or("replay stream absent")?;
    if receipt.stream != expected.config().name || receipt.sequence == 0 {
        return Err("invalid replay receipt".into());
    }
    if pause {
        // Application result loss, not wire ACK loss. The parent can kill this
        // owned child after observing a real persisted broker receipt.
        println!("persisted");
        std::io::stdout().flush()?;
        if input.read_u8().await? != b'c' {
            return Err("invalid receipt release".into());
        }
    }
    persistence::complete(database, &actor, &claim, &receipt).await?;
    Ok(if receipt.duplicate {
        "replay_duplicate"
    } else {
        "replayed"
    })
}
