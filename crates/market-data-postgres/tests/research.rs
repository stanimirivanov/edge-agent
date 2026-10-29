//! Owner correlation rollback and forged-ready refusal in private PostgreSQL schemas.
#![allow(
    clippy::disallowed_methods,
    reason = "Isolated test schema inspection and fault injection."
)]
#[allow(dead_code)]
mod support;
use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    research::{AbstentionReason, ResearchRecord, ResearchRequest},
    research_acquisition::{self, ResearchEvidence},
};
use edgeagent_market_data_application::{AcquisitionLedger, Cancellation};
use edgeagent_market_data_postgres::{PostgresAcquisitionLedger, PostgresResearchEvidence};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use support::*;
fn delivery(request: &ResearchRequest) -> Result<MessageEnvelope> {
    let (key, partition) = research_acquisition::binding(request)?;
    let def = research_acquisition::REQUEST;
    Ok(MessageEnvelope::from_payload(
        MessageMetadata {
            id: "delivery-1".into(),
            source: Component::Gateway.source_uri().into(),
            message_type: def.message_type().into(),
            subject: partition.clone(),
            time: "2026-10-29T00:00:00Z".into(),
            data_schema: def.data_schema().into(),
            correlation_id: key.clone(),
            causation_id: "cause-1".into(),
            idempotency_key: key,
            partition_key: partition,
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
            trace_state: None,
        },
        &ResearchRecord::Request(request.clone()),
    )?)
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn correlation_commit_failures_preserve_pending_work_without_inbox_or_outbox() -> Result {
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        let case=Case::new(h.now,1)?;let record=case.record()?;
        PostgresAcquisitionLedger::new(h.pool.clone()).admit(&record,&case.catalogs().admit(&case.request,case.request.admitted_at,&Cancellation::default())?).await?;
        let store=PostgresResearchEvidence::new(h.pool.clone());
        let request=ResearchRequest{scope:"operator".into(),request_id:"research-1".into(),implementation:"02".repeat(32),configuration:"03".repeat(32),decision_at_ms:h.now,lookback_sessions:6,instruments:vec![format!("{:032x}",1)]};
        let delivery=delivery(&request)?;store.reserve(&delivery,&record).await?;
        let mut changed:serde_json::Value=serde_json::from_slice(&delivery.to_json()?)?;
        changed["time"]="2026-10-29T00:00:01Z".into();
        assert!(store.reserve(&MessageEnvelope::from_json(&serde_json::to_vec(&changed)?)?,&record).await.is_err());
        let fake_object:edgeagent_contracts::evidence::EvidenceObject=serde_json::from_value(serde_json::json!({
            "sha256":"04".repeat(32),"bytes":1,"storage":"00000000000000000000000000000001",
            "audience":"00000000000000000000000000000001","rights":{"id":"00000000000000000000000000000001","revision":"1"},"delete_by":{"seconds":1793290000,"nanos":0},"replaces":null
        }))?;
        let fake=ResearchEvidence::Ready{request:request.clone(),decision:Box::new(edgeagent_contracts::evidence::EvidenceDecisionData {request:record.command().clone(),outcome:edgeagent_contracts::evidence::EvidenceOutcome::Ready{object:fake_object}})};
        assert!(store.complete(&delivery,&fake).await.is_err());
        let outcome=ResearchEvidence::Abstained{request:request.clone(),reason:AbstentionReason::UnsupportedUniverse};
        sqlx::raw_sql("CREATE FUNCTION test_refuse_correlation() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'private-test-sentinel'; END; $$").execute(&h.pool).await?;
        for trigger in ["CREATE TRIGGER test_refuse BEFORE INSERT ON edgeagent_message_outbox FOR EACH ROW EXECUTE FUNCTION test_refuse_correlation()","CREATE CONSTRAINT TRIGGER test_refuse AFTER INSERT ON edgeagent_message_outbox DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION test_refuse_correlation()"] {
            sqlx::raw_sql(trigger).execute(&h.pool).await?;
            let error=store.complete(&delivery,&outcome).await.err().ok_or("expected failure")?;
            assert!(!format!("{error:?}").contains("private-test-sentinel"));
            assert!(store.reserve(&delivery,&record).await?.publication.is_none());
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_message_inbox").fetch_one(&h.pool).await?,0);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_message_outbox").fetch_one(&h.pool).await?,0);
            sqlx::raw_sql("DROP TRIGGER test_refuse ON edgeagent_message_outbox").execute(&h.pool).await?;
        }
        let event=store.complete(&delivery,&outcome).await?;
        assert_eq!(store.complete(&delivery,&outcome).await?.to_json()?,event.to_json()?);
        sqlx::query("DELETE FROM edgeagent_message_outbox").execute(&h.pool).await?;
        assert!(store.reserve(&delivery,&record).await.is_err());Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
