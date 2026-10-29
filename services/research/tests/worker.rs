//! Real completion and owner-process behavior in unique private PostgreSQL schemas.
#![allow(
    clippy::disallowed_methods,
    reason = "Isolated schema inspection/fault injection."
)]
#[path = "support/database.rs"]
mod database;
#[allow(dead_code)]
mod support;
use database::Database;
use edgeagent_contracts::research::ResearchRecord;
use futures_util::FutureExt;
use support::*;
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn restart_and_lost_result_recover_before_owner_read_with_one_publication() -> Result {
    let db = Database::new().await?;
    let result = std::panic::AssertUnwindSafe(async {
        let (event, bytes) = event("request-1")?;
        let profile = Profile::new(&bytes)?;
        let first = run(&profile, &db.schema, &event.to_json()?).await?;
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        assert!(matches!(
            ResearchRecord::decode(&first.stdout)?,
            ResearchRecord::Artifact(_)
        ));
        profile.unavailable()?;
        std::fs::remove_file(&profile.owner)?;
        // Lost result and all process state; the second process must not need its failed owner.
        let again = run(&profile, &db.schema, &event.to_json()?).await?;
        assert!(again.status.success());
        assert_eq!(first.stdout, again.stdout);
        for table in [
            "edgeagent_research_completions",
            "edgeagent_message_outbox",
            "edgeagent_message_inbox",
        ] {
            let count: i64 =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(&db.pool)
                    .await?;
            assert_eq!(count, 1);
        }
        let publication: Vec<u8> =
            sqlx::query_scalar("SELECT publication FROM edgeagent_research_completions")
                .fetch_one(&db.pool)
                .await?;
        assert_eq!(
            edgeagent_contracts::MessageEnvelope::from_json(&publication)?
                .payload::<ResearchRecord>()?
                .canonical_bytes()?,
            first.stdout
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    db.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn corrupt_and_denied_reads_abstain_while_unavailable_reads_leave_no_completion() -> Result {
    let db = Database::new().await?;
    let result=std::panic::AssertUnwindSafe(async {
        let (event,bytes)=event("corrupt")?;let profile=Profile::new(&bytes)?;
        std::fs::write(&profile.artifact,b"corrupt invented bytes")?;
        let output=run(&profile,&db.schema,&event.to_json()?).await?;assert!(output.status.success());
        assert!(matches!(ResearchRecord::decode(&output.stdout)?,ResearchRecord::Abstention(a) if a.reason==edgeagent_contracts::research::AbstentionReason::InvalidEvidence));
        let (denied,_)=support::event("denied")?;std::fs::write(&profile.owner,"#!/bin/sh\nexit 77\n")?;
        let output=run(&profile,&db.schema,&denied.to_json()?).await?;assert!(output.status.success());
        assert!(matches!(ResearchRecord::decode(&output.stdout)?,ResearchRecord::Abstention(a) if a.reason==edgeagent_contracts::research::AbstentionReason::MissingEntitlement));
        let mut legacy:serde_json::Value=serde_json::from_slice(&denied.to_json()?)?;
        let mut request=edgeagent_research_application::publication::evaluation_request(&denied)?;request.request_id="legacy".into();
        let record=ResearchRecord::Request(request.clone());let (key,partition)=edgeagent_contracts::research_acquisition::binding(&request)?;
        legacy["type"]=edgeagent_research_application::publication::EVALUATE.message_type().into();legacy["source"]=edgeagent_contracts::Component::Gateway.source_uri().into();legacy["dataschema"]=edgeagent_research_application::publication::EVALUATE.data_schema().into();legacy["data"]=serde_json::to_value(record)?;
        legacy["id"]=key.clone().into();legacy["correlationid"]=key.clone().into();legacy["idempotencykey"]=key.into();legacy["subject"]=partition.clone().into();legacy["partitionkey"]=partition.into();profile.unavailable()?;
        let output=run(&profile,&db.schema,&serde_json::to_vec(&legacy)?).await?;assert!(output.status.success());
        assert!(matches!(ResearchRecord::decode(&output.stdout)?,ResearchRecord::Abstention(a) if a.reason==edgeagent_contracts::research::AbstentionReason::MissingEntitlement));
        std::fs::write(&profile.artifact,vec![b' ';edgeagent_contracts::evidence::MAX_EVIDENCE_BYTES+1])?;
        std::fs::write(&profile.owner,format!("#!/bin/sh\n/bin/cat '{}'\n",profile.artifact.display()))?;
        let (oversized,_)=support::event("oversized")?;
        let output=run(&profile,&db.schema,&oversized.to_json()?).await?;assert!(output.status.success());
        assert!(matches!(ResearchRecord::decode(&output.stdout)?,ResearchRecord::Abstention(a) if a.reason==edgeagent_contracts::research::AbstentionReason::InvalidEvidence));
        let (unavailable,_)=support::event("unavailable")?;profile.unavailable()?;
        assert!(!run(&profile,&db.schema,&unavailable.to_json()?).await?.status.success());
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM edgeagent_research_completions").fetch_one(&db.pool).await?,4);Ok(())
    }).catch_unwind().await;
    db.finish(result).await
}
