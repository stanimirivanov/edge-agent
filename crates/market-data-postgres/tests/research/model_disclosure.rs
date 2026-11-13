use super::*;
#[allow(dead_code)]
#[path = "../../../market-data-application/tests/support/publication.rs"]
mod fixtures;
use edgeagent_contracts::{
    evidence::*,
    explanation::Anchor,
    model_disclosure::{Purpose, Request},
};
use edgeagent_market_data_application::*;
use edgeagent_market_data_postgres::ModelDisclosureReview;
use fixtures::{Access, Storage, prepare};
use std::sync::atomic::Ordering;

async fn ready(h: &Harness) -> Result<(Request, AdmissionRecord, Access)> {
    let case = Case::new(h.now, 1)?;
    let record = case.record()?;
    let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
    let cancel = Cancellation::default();
    let token = case
        .catalogs()
        .admit(&case.request, case.request.admitted_at, &cancel)?;
    ledger.admit(&record, &token).await?;
    let access = Access::new(h.now)?;
    let storage = Storage::default();
    let proposal = prepare(
        &record,
        &token,
        case.catalogs(),
        &RetainedEvidenceStorage::new(&storage, &ledger),
        &access,
        None,
        100,
    )
    .await?;
    let receipt = ledger
        .commit(
            &fixtures::delivery(record.command(), "acquire")?,
            &proposal,
            &access,
        )
        .await?;
    let research = ResearchRequest {
        scope: "operator".into(),
        request_id: "model-check".into(),
        implementation: "02".repeat(32),
        configuration: "03".repeat(32),
        decision_at_ms: h.now,
        lookback_sessions: 6,
        instruments: vec![format!("{:032x}", 1)],
    };
    let bridge = PostgresResearchEvidence::new(h.pool.clone());
    let ingress = delivery(&research)?;
    bridge.reserve(&ingress, &record).await?;
    let event = bridge
        .complete(
            &ingress,
            &ResearchEvidence::Ready {
                request: research.clone(),
                decision: Box::new(receipt.decision().data().clone()),
            },
        )
        .await?;
    let EvidenceOutcome::Ready { object } = &receipt.decision().data().outcome else {
        return Err("ready".into());
    };
    let profile = serde_json::from_value(
        serde_json::json!({"deployment":{"runtime":"a".repeat(64),"weights":"b".repeat(64)},"settings":{"seed":1,"temperature_milli":0,"max_output_tokens":512,"timeout_ms":1000},"prompt":"c".repeat(64),"validator":"d".repeat(64),"renderer":"e".repeat(64),"tools":"none"}),
    )?;
    Ok((
        Request {
            operation: EvidenceId::new(&"1".repeat(32))?,
            anchor: Anchor {
                scope: research.scope.clone(),
                artifact: "a".repeat(64),
                decision: "b".repeat(64),
            },
            research,
            acquisition: record.command().request.clone(),
            publication: DailyEvidenceArtifact::hash(&event.to_json()?)
                .as_str()
                .into(),
            evidence: object.sha256.clone(),
            audience: "local-model".into(),
            purpose: Purpose::ModelDisclosure,
            profile,
            input_contract: "e".repeat(64),
            input: "f".repeat(64),
        },
        record,
        access,
    ))
}
fn review(
    request: &Request,
    revision: i64,
    decision: &str,
    from: i64,
    until: i64,
) -> Result<ModelDisclosureReview> {
    let value = serde_json::json!({"request":request,"revision":revision,"decision":decision,"agreement":"3".repeat(32),"valid_from_ms":from,"expires_at_ms":until});
    let mut bytes = edgeagent_contracts::canonical_json_bytes(&value)?;
    bytes.push(b'\n');
    Ok(ModelDisclosureReview::decode(&bytes)?)
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn model_reviews_are_separate_current_and_exactly_bound() -> Result {
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        let (request,record,access)=ready(&h).await?;let bridge=PostgresResearchEvidence::new(h.pool.clone());
        assert!(bridge.model_disclosure_permit(&request,&access).await.is_err());
        let serving=edgeagent_market_data_postgres::ExplanationServingReview::decode(&serde_json::to_vec(&serde_json::json!({"audience":"4".repeat(32),"agreement":"3".repeat(32),"revision":1,"decision":"allow","valid_from_ms":h.now-1000,"expires_at_ms":h.now+900000}))?)?;
        bridge.review_explanation_serving(&request.acquisition,&serving).await?;
        assert!(bridge.model_disclosure_permit(&request,&access).await.is_err());
        bridge.review_model_disclosure(&review(&request,1,"allow",h.now-1000,h.now+900000)?).await?;
        let permit=bridge.model_disclosure_permit(&request,&access).await?;
        permit.bind(&request,permit.checked_at_ms)?;
        assert_eq!(permit.expires_at_ms,record.delete_by_ms()?);
        assert_eq!(permit.review_revision,1);
        let fields:serde_json::Value=serde_json::from_slice(record.bytes())?;
        let names=["rights","principal","account","source","instrument","listing","endpoint","audience","storage","semantics_id","semantics_revision","precision","market_scope"];
        let data:serde_json::Map<String,serde_json::Value>=names.iter().map(|name|((*name).into(),fields[*name].clone())).collect();
        let mut scope=edgeagent_contracts::canonical_json_bytes(&serde_json::json!({"kind":"market-data.model-disclosure-rights-scope.v1","data":data}))?;scope.push(b'\n');
        assert_eq!(permit.rights_scope,DailyEvidenceArtifact::hash(&scope).as_str());
        for field in 0..6 {
            let mut foreign=request.clone();match field {0=>foreign.publication="4".repeat(64),1=>foreign.evidence=EvidenceDigest::new(&"4".repeat(64))?,2=>foreign.acquisition=EvidenceId::new(&"4".repeat(32))?,3=>foreign.profile.deployment.weights="4".repeat(64),4=>foreign.input_contract="4".repeat(64),_=>foreign.research.request_id="foreign".into()};
            assert!(bridge.model_disclosure_permit(&foreign,&access).await.is_err(),"foreign field {field}");
        }
        let mut next=request.clone();next.operation=EvidenceId::new(&"5".repeat(32))?;next.input="5".repeat(64);
        let fresh=bridge.model_disclosure_permit(&next,&access).await?;assert!(fresh.bind(&request,fresh.checked_at_ms).is_err());
        access.denied.store(true,Ordering::SeqCst);assert!(bridge.model_disclosure_permit(&request,&access).await.is_err());access.denied.store(false,Ordering::SeqCst);
        for (revision,decision,from,until) in [(2,"deny",h.now-1000,h.now+100000),(3,"allow",h.now+100000,h.now+200000),(4,"allow",h.now-2000,h.now-1000),(5,"revoke",h.now-1000,h.now+100000)] {
            bridge.review_model_disclosure(&review(&request,revision,decision,from,until)?).await?;
            assert!(bridge.model_disclosure_permit(&request,&access).await.is_err());
        }
        bridge.review_model_disclosure(&review(&request,6,"allow",h.now-1000,h.now+100000)?).await?;
        let pool=h.reconnect().await?;let restarted=PostgresResearchEvidence::new(pool.clone());
        assert_eq!(restarted.model_disclosure_permit(&request,&access).await?.review_revision,6);
        sqlx::query("DELETE FROM edgeagent_message_outbox WHERE message_type=$1").bind(research_acquisition::DECIDED.message_type()).execute(&h.pool).await?;
        assert!(restarted.model_disclosure_permit(&request,&access).await.is_err());pool.close().await;
        Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn model_review_administration_is_audited_serialized_and_append_only() -> Result {
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        let (request,_,access)=ready(&h).await?;let bridge=PostgresResearchEvidence::new(h.pool.clone());
        let allow=review(&request,1,"allow",h.now-1000,h.now+100000)?;
        assert!(bridge.review_model_disclosure(&review(&request,2,"allow",h.now-1000,h.now+100000)?).await.is_err());
        bridge.review_model_disclosure(&allow).await?;bridge.review_model_disclosure(&allow).await?;
        assert!(bridge.review_model_disclosure(&review(&request,1,"deny",h.now-1000,h.now+100000)?).await.is_err());
        let (worker,role)=h.worker().await?;let guest=PostgresResearchEvidence::new(worker.clone());
        assert!(guest.review_model_disclosure(&allow).await.is_err());
        let refused=sqlx::query("INSERT INTO edgeagent_model_disclosure_reviews SELECT review_key,rights_scope,2,request,decision,agreement,valid_from_ms,expires_at_ms,actor,recorded_at FROM edgeagent_model_disclosure_reviews").execute(&worker).await.err().ok_or("nonowner insert")?;
        assert_eq!(refused.as_database_error().and_then(|e|e.code()).as_deref(),Some("42501"));
        assert!(sqlx::query("UPDATE edgeagent_model_disclosure_reviews SET decision='deny'").execute(&h.pool).await.is_err());
        assert!(sqlx::query("DELETE FROM edgeagent_model_disclosure_reviews").execute(&h.pool).await.is_err());
        assert!(sqlx::query("TRUNCATE edgeagent_model_disclosure_reviews").execute(&h.pool).await.is_err());
        let deny=review(&request,2,"deny",h.now-1000,h.now+100000)?;let revoke=review(&request,2,"revoke",h.now-1000,h.now+100000)?;
        let (a,b)=tokio::join!(bridge.review_model_disclosure(&deny),bridge.review_model_disclosure(&revoke));assert_ne!(a.is_ok(),b.is_ok());
        let audited:bool=sqlx::query_scalar("SELECT count(*)=2 AND bool_and(actor=session_user AND recorded_at IS NOT NULL) FROM edgeagent_model_disclosure_reviews").fetch_one(&h.pool).await?;assert!(audited);
        assert!(bridge.model_disclosure_permit(&request,&access).await.is_err());
        bridge.review_model_disclosure(&review(&request,3,"allow",h.now-1000,h.now+100000)?).await?;
        bridge.model_disclosure_permit(&request,&access).await?;
        let admission=bridge.owner_record(&request.acquisition).await?;
        PostgresAcquisitionLedger::new(h.pool.clone()).revoke_evidence_scope(&admission.retention_scope()?).await?;
        assert!(bridge.model_disclosure_permit(&request,&access).await.is_err());
        h.drop_worker(worker,&role).await?;Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn model_permit_waits_for_review_commit_and_observes_revocation() -> Result {
    let h = Harness::new().await?;
    let result=AssertUnwindSafe(async {
        let (request,_,_)=ready(&h).await?;let bridge=PostgresResearchEvidence::new(h.pool.clone());
        bridge.review_model_disclosure(&review(&request,1,"allow",h.now-1000,h.now+100000)?).await?;
        let mut tx=h.pool.begin().await?;
        let blocker:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO edgeagent_model_disclosure_reviews SELECT review_key,rights_scope,2,request,'revoke',agreement,valid_from_ms,expires_at_ms,actor,recorded_at FROM edgeagent_model_disclosure_reviews WHERE revision=1").execute(&mut *tx).await?;
        let pool=h.reconnect().await?;let remote=PostgresResearchEvidence::new(pool.clone());let access=Access::new(h.now)?;
        let check=remote.model_disclosure_permit(&request,&access);tokio::pin!(check);
        let blocked=async {
            loop {
                let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))").bind(blocker).fetch_one(&h.pool).await?;
                if waiting {return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());}
                tokio::task::yield_now().await;
            }
        };
        tokio::select! {
            result=&mut check=>return Err(format!("permit completed before review commit: {}",result.is_ok()).into()),
            result=tokio::time::timeout(std::time::Duration::from_secs(3),blocked)=>result??,
        }
        tx.commit().await?;
        assert!(check.await.is_err());pool.close().await;Ok(())
    }).catch_unwind().await;
    h.finish(result).await
}
