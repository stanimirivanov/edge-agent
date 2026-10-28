use super::*;
use std::num::NonZeroU64;
fn second_account(ms: i64) -> Result<fixture::Fixture> {
    let mut f = fixture::Fixture::new(ms, 1)?;
    let account = ProviderAccountId::new(2)?;
    for scope in &mut f.scopes {
        scope.account = account;
    }
    f.rights = ReferenceHistory::new(
        f.rights
            .at(fixture::at(ms)?)
            .map(|old| -> Result<_> {
                let mut value = *old.value();
                value.scope.account = account;
                Ok(ReferenceVersion::new(
                    ReferenceId::new(old.id().get() + 1000)?,
                    old.revision(),
                    old.source(),
                    old.known_at(),
                    *old.checksum(),
                    value,
                ))
            })
            .collect::<Result<Vec<_>>>()?,
    )?;
    f.providers = ReferenceHistory::new(
        f.providers
            .at(fixture::at(ms)?)
            .map(|old| -> Result<_> {
                let mut value = *old.value();
                value.scope.account = account;
                Ok(ReferenceVersion::new(
                    ReferenceId::new(old.id().get() + 1000)?,
                    old.revision(),
                    old.source(),
                    old.known_at(),
                    *old.checksum(),
                    value,
                ))
            })
            .collect::<Result<Vec<_>>>()?,
    )?;
    Ok(f)
}
pub(super) fn record(
    f: &fixture::Fixture,
    id: u128,
    namespace: u64,
    scope: &[u8],
) -> Result<(StreamingSession, StreamOwnerInputs)> {
    let profile = b"invented complete private profile";
    let mut data = f.data(id)?;
    data.configuration = DailyEvidenceArtifact::hash(profile);
    let s = f.seal(data)?;
    let claim = StreamNamespaceClaim::new(NonZeroU64::new(namespace).ok_or("namespace")?, scope)?;
    let inputs = StreamOwnerInputs::new(&s, profile, &[claim], fixture::at(f.ms + 600_000)?)?;
    Ok((s, inputs))
}
#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_owner_inputs_require_independent_roles_and_exact_current_loading() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result = AssertUnwindSafe(async {
        let f = fixture::Fixture::new(h.now - 100, 1)?;
        let (s, inputs) = record(&f, 1, u64::MAX, br#"["source",1]"#)?;
        let owner = Ledger::new(h.pool.clone());
        let worker = Ledger::new(pool.clone());
        let cancel = Cancellation::default();
        owner.configure_account(f.account()?, f.policy(1, 4)?, 0).await?;
        owner.admit(&s, f.catalogs()).await?;
        assert!(worker.load_stream_inputs(&s, &inputs, f.catalogs(), &cancel).await.is_err());
        assert!(worker.install_stream_inputs(&s, &inputs, f.catalogs()).await.is_err());
        owner.install_stream_inputs(&s, &inputs, f.catalogs()).await?;
        owner.install_stream_inputs(&s, &inputs, f.catalogs()).await?;
        assert_eq!(worker.load_stream_inputs(&s, &inputs, f.catalogs(), &cancel).await?, inputs);
        let changed = StreamOwnerInputs::new(
            &s, inputs.profile(), inputs.claims(),
            fixture::at(inputs.expires_at().unix_millis() - 1)?,
        )?;
        kind(worker.load_stream_inputs(&s, &changed, f.catalogs(), &cancel).await, BudgetErrorKind::Conflict)?;
        assert!(sqlx::query("UPDATE edgeagent_stream_owner_inputs SET inputs=NULL,retired=TRUE")
            .execute(&pool).await.is_err());
        assert!(sqlx::query("SELECT edgeagent_stream_owner_inputs_control($1,'install',$2,$3)")
            .bind(s.data().id.as_str()).bind(inputs.bytes())
            .bind(inputs.expires_at().unix_millis()).execute(&pool).await.is_err());
        // Exercise database expiry independently of Rust's own authority recheck.
        sqlx::query("UPDATE edgeagent_stream_owner_inputs SET expires_ms=0")
            .execute(&h.pool).await?;
        let expired: Option<Vec<u8>> = sqlx::query_scalar("SELECT edgeagent_load_stream_inputs($1)")
            .bind(s.data().id.as_str()).fetch_one(&pool).await?;
        assert!(expired.is_none());
        sqlx::query("UPDATE edgeagent_stream_owner_inputs SET expires_ms=$1")
            .bind(inputs.expires_at().unix_millis()).execute(&h.pool).await?;
        // Temporary shadow relations cannot supply an approval or replace the real record.
        sqlx::raw_sql("CREATE TEMP TABLE edgeagent_stream_owner_inputs(session_id TEXT,inputs BYTEA);")
            .execute(&pool).await?;
        assert_eq!(worker.load_stream_inputs(&s, &inputs, f.catalogs(), &cancel).await?, inputs);
        let reviewer = format!("{role}_reviewer");
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE ROLE {reviewer} NOLOGIN; GRANT UPDATE ON edgeagent_stream_namespaces TO {reviewer}; GRANT {reviewer} TO {role};"
        ))).execute(&h.pool).await?;
        assert!(worker.load_stream_inputs(&s, &inputs, f.catalogs(), &cancel).await.is_err());
        Ok(())
    }).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_namespace_collisions_and_changed_retries_roll_back_every_claim() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result = AssertUnwindSafe(async {
        let f = fixture::Fixture::new(h.now - 100, 1)?;
        let owner = Ledger::new(h.pool.clone());
        let worker = Ledger::new(pool.clone());
        owner
            .configure_account(f.account()?, f.policy(1, 4)?, 0)
            .await?;
        let (a, one) = record(&f, 1, 7, br#"["source",1]"#)?;
        let other = second_account(h.now - 100)?;
        owner
            .configure_account(ProviderAccountId::new(2)?, other.policy(1, 4)?, 0)
            .await?;
        let (b, two) = record(&other, 2, 7, br#"["source",2]"#)?;
        owner.admit(&a, f.catalogs()).await?;
        owner.admit(&b, other.catalogs()).await?;
        let (left, right) = tokio::join!(
            owner.install_stream_inputs(&a, &one, f.catalogs()),
            owner.install_stream_inputs(&b, &two, other.catalogs())
        );
        assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
        let (winning, installed, catalogs) = if left.is_ok() {
            (&a, &one, f.catalogs())
        } else {
            (&b, &two, other.catalogs())
        };
        let claims = [StreamNamespaceClaim::new(
            NonZeroU64::new(8).ok_or("namespace")?,
            br#"["unused"]"#,
        )?];
        let changed = StreamOwnerInputs::new(
            winning,
            installed.profile(),
            &claims,
            installed.expires_at(),
        )?;
        kind(
            owner
                .install_stream_inputs(winning, &changed, catalogs)
                .await,
            BudgetErrorKind::Conflict,
        )?;
        // Direct/autocommit SQL must abort all reservations too, without Rust rollback.
        assert!(
            sqlx::query("SELECT edgeagent_stream_owner_inputs_control($1,'install',$2,$3)")
                .bind(winning.data().id.as_str())
                .bind(changed.bytes())
                .bind(changed.expires_at().unix_millis())
                .execute(&h.pool)
                .await
                .is_err()
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM edgeagent_stream_namespaces")
            .fetch_one(&h.pool)
            .await?;
        assert_eq!(count, 1);
        let (c, reverse) = record(&f, 3, 9, installed.claims().first().ok_or("claim")?.scope())?;
        owner.admit(&c, f.catalogs()).await?;
        kind(
            owner
                .install_stream_inputs(&c, &reverse, f.catalogs())
                .await,
            BudgetErrorKind::Conflict,
        )?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM edgeagent_stream_owner_inputs")
            .fetch_one(&h.pool)
            .await?;
        assert_eq!(count, 1);
        assert_eq!(
            worker
                .load_stream_inputs(winning, installed, catalogs, &Cancellation::default())
                .await?,
            *installed
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
#[tokio::test]
#[ignore = "requires isolated local PostgreSQL"]
async fn stream_input_retirement_erases_private_scope_without_reopening_namespaces() -> Result {
    let h = Harness::new().await?;
    let (pool, role) = h.worker().await?;
    let result = AssertUnwindSafe(async {
        let f = fixture::Fixture::new(h.now - 100, 1)?;
        let owner = Ledger::new(h.pool.clone());
        let worker = Ledger::new(pool.clone());
        owner.configure_account(f.account()?, f.policy(1, 4)?, 0).await?;
        let (a, one) = record(&f, 1, 7, br#"["source",1]"#)?;
        let (b, two) = record(&f, 2, 7, br#"["source",1]"#)?;
        owner.admit(&a, f.catalogs()).await?;
        owner.admit(&b, f.catalogs()).await?;
        owner.install_stream_inputs(&a, &one, f.catalogs()).await?;
        owner.install_stream_inputs(&b, &two, f.catalogs()).await?;
        assert!(owner.retire_stream_inputs(f.account()?, &a.data().id).await.is_err());
        let connection = worker.reserve(&a, f.catalogs(), &Cancellation::default()).await?;
        let before = worker.status(&a.data().id).await?;
        owner.confirm_stream_writers_quiescent(f.account()?, &a.data().id).await?;
        owner.retire_stream_inputs(f.account()?, &a.data().id).await?;
        assert!(worker.load_stream_inputs(&a, &one, f.catalogs(), &Cancellation::default()).await.is_err());
        assert_eq!(worker.load_stream_inputs(&b, &two, f.catalogs(), &Cancellation::default()).await?, two);
        let kept: bool = sqlx::query_scalar(
            "SELECT scope IS NOT NULL FROM edgeagent_stream_namespaces WHERE namespace='0000000000000007'"
        ).fetch_one(&h.pool).await?;
        assert!(kept);
        owner.confirm_stream_writers_quiescent(f.account()?, &b.data().id).await?;
        owner.retire_stream_inputs(f.account()?, &b.data().id).await?;
        owner.retire_stream_inputs(f.account()?, &b.data().id).await?;
        let erased: (bool, bool) = sqlx::query_as(
            "SELECT (SELECT bool_and(inputs IS NULL AND retired) FROM edgeagent_stream_owner_inputs),(SELECT scope IS NULL AND retired FROM edgeagent_stream_namespaces WHERE namespace='0000000000000007')"
        ).fetch_one(&h.pool).await?;
        assert_eq!(erased, (true, true));
        let (c, again) = record(&f, 3, 7, br#"["different",3]"#)?;
        owner.admit(&c, f.catalogs()).await?;
        kind(owner.install_stream_inputs(&c, &again, f.catalogs()).await, BudgetErrorKind::Conflict)?;
        let after = worker.status(&a.data().id).await?;
        assert_eq!(after.usage.starts, before.usage.starts);
        assert!(after.unresolved);
        assert_eq!(after.generation, connection.generation());
        Ok(())
    }).catch_unwind().await;
    h.drop_worker(pool, &role).await?;
    h.finish(result).await
}
