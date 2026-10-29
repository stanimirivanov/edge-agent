//! Separate worker-process proofs over invented data and an owned PostgreSQL schema.
#![allow(
    clippy::disallowed_methods,
    reason = "Owned test schema inspection and cleanup."
)]
use edgeagent_contracts::{
    evidence::*, research::AbstentionReason, research_acquisition::ResearchEvidence,
};
use edgeagent_market_data_application::AcquisitionLedger;
use edgeagent_market_data_postgres::{PostgresAcquisitionLedger, PostgresResearchEvidence};
use futures_util::{FutureExt, TryStreamExt};
mod research_support;
use research_support::*;
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn pending_research_restarts_and_reordered_duplicates_reuse_one_acquisition() -> Result {
    let h = Harness::new().await?;
    let result = std::panic::AssertUnwindSafe(async {
        let request = h.seed("history-v1").await?;
        let first = delivery(&request, "delivery-1")?;
        let bridge = PostgresResearchEvidence::new(h.pool.clone());
        let owner = bridge
            .owner_record(&EvidenceId::new("00000000000000000000000000000001")?)
            .await?;
        // Persist a pending binding, then lose all worker state before starting another process.
        assert!(bridge.reserve(&first, &owner).await?.publication.is_none());
        // Simulate restart after the acquisition attempt marker as well as correlation reservation.
        PostgresAcquisitionLedger::new(h.pool.clone())
            .begin_attempt(owner.command(), 1)
            .await?;
        let one = h.once(&first).await?;
        assert!(matches!(
            ResearchEvidence::from_envelope(&one)?,
            ResearchEvidence::Ready { .. }
        ));
        let duplicate = delivery(&request, "delivery-2")?;
        assert_eq!(h.once(&duplicate).await?.to_json()?, one.to_json()?);
        assert_eq!(h.once(&first).await?.to_json()?, one.to_json()?);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_acquisitions")
                .fetch_one(&h.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_research_evidence")
                .fetch_one(&h.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_message_outbox")
                .fetch_one(&h.pool)
                .await?,
            2
        );
        let objects =
            object_store::local::LocalFileSystem::new_with_prefix(h.root.join("objects"))?;
        use object_store::ObjectStore as _;
        assert_eq!(objects.list(None).try_collect::<Vec<_>>().await?.len(), 1);
        let mut changed = request.clone();
        changed.lookback_sessions = 4;
        assert!(
            !h.run(
                &["offline-research-once", "00000000000000000000000000000001"],
                Some(&delivery(&changed, "changed")?.to_json()?)
            )
            .await?
            .status
            .success()
        );
        let mut wrong: serde_json::Value = serde_json::from_slice(&first.to_json()?)?;
        wrong["correlationid"] = "foreign".into();
        assert!(
            !h.run(
                &["offline-research-once", "00000000000000000000000000000001"],
                Some(&serde_json::to_vec(&wrong)?)
            )
            .await?
            .status
            .success()
        );
        assert!(
            sqlx::query("DELETE FROM edgeagent_research_evidence")
                .execute(&h.pool)
                .await
                .is_err()
        );
        let ledger = PostgresAcquisitionLedger::new(h.pool.clone());
        ledger
            .revoke_evidence_scope(&owner.retention_scope()?)
            .await?;
        ledger.retire_due_evidence(16).await?;
        assert!(bridge.denied(&first).await?);
        for table in [
            "edgeagent_research_evidence",
            "edgeagent_message_outbox",
            "edgeagent_message_inbox",
        ] {
            let count: i64 =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(&h.pool)
                    .await?;
            assert_eq!(count, 0, "retirement erases {table}");
        }
        h.success(
            &[
                "offline-seed",
                "00000000000000000000000000000002",
                "history-v1",
            ],
            None,
        )
        .await?;
        let another = bridge
            .owner_record(&EvidenceId::new("00000000000000000000000000000002")?)
            .await?;
        assert!(bridge.reserve(&first, &another).await.is_err());
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn unsupported_selection_and_revoked_owner_rights_abstain_without_objects() -> Result {
    let h = Harness::new().await?;
    let result = std::panic::AssertUnwindSafe(async {
        let mut request = h.seed("history-v1").await?;
        request.lookback_sessions = 6;
        let event = h.once(&delivery(&request, "unsupported")?).await?;
        assert!(matches!(
            ResearchEvidence::from_envelope(&event)?,
            ResearchEvidence::Abstained {
                reason: AbstentionReason::UnsupportedUniverse,
                ..
            }
        ));
        let record = PostgresResearchEvidence::new(h.pool.clone())
            .owner_record(&EvidenceId::new("00000000000000000000000000000001")?)
            .await?;
        PostgresAcquisitionLedger::new(h.pool.clone())
            .revoke_evidence_scope(&record.retention_scope()?)
            .await?;
        request.request_id = "denied".into();
        request.lookback_sessions = 5;
        let event = h.once(&delivery(&request, "denied")?).await?;
        assert!(matches!(
            ResearchEvidence::from_envelope(&event)?,
            ResearchEvidence::Abstained {
                reason: AbstentionReason::MissingEntitlement,
                ..
            }
        ));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_acquisition_receipts")
                .fetch_one(&h.pool)
                .await?,
            0
        );
        use object_store::ObjectStore as _;
        let objects =
            object_store::local::LocalFileSystem::new_with_prefix(h.root.join("objects"))?;
        assert_eq!(objects.list(None).try_collect::<Vec<_>>().await?.len(), 0);
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL via EDGEAGENT_POSTGRES_URL"]
async fn incomplete_owner_history_abstains_without_a_ready_reference() -> Result {
    let h = Harness::new().await?;
    let result = std::panic::AssertUnwindSafe(async {
        let request = h.seed("history-missing-v1").await?;
        let event = h.once(&delivery(&request, "missing-history")?).await?;
        assert!(matches!(
            ResearchEvidence::from_envelope(&event)?,
            ResearchEvidence::Abstained {
                reason: AbstentionReason::InvalidEvidence,
                ..
            }
        ));
        assert_eq!(
            h.once(&delivery(&request, "retry-missing")?)
                .await?
                .to_json()?,
            event.to_json()?
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM edgeagent_acquisition_receipts")
                .fetch_one(&h.pool)
                .await?,
            1
        );
        use object_store::ObjectStore as _;
        let objects =
            object_store::local::LocalFileSystem::new_with_prefix(h.root.join("objects"))?;
        assert_eq!(objects.list(None).try_collect::<Vec<_>>().await?.len(), 0);
        Ok(())
    })
    .catch_unwind()
    .await;
    h.finish(result).await
}
