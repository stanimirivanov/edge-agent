use super::*;
impl StreamingLedger for PostgresStreamingLedger {
    async fn reserve(
        &self,
        session: &StreamingSession,
        catalogs: AuthorityCatalogs<'_>,
        cancellation: &Cancellation,
    ) -> Result<StreamingConnection> {
        let mut tx = self.ledger.transaction().await?;
        let account = key(session.account());
        let policy = lock_account(&mut tx, &account).await?;
        worker(&mut tx, session).await?;
        recovery::enforce(&mut tx, session, self.fence.as_ref(), false).await?;
        let state = state(&mut tx, &session.data().id).await?;
        let at = now(&mut tx).await?;
        validate_policy(&policy, session, at)?;
        session.recheck(catalogs, at, cancellation)?;
        let usage = state.status()?.usage;
        if state.active {
            return Err(Error::new(K::Busy));
        }
        if state.stopped {
            return Err(Error::new(K::Exhausted));
        }
        let active=sqlx::query_scalar!(r#"SELECT count(*) AS "count!" FROM edgeagent_stream_sessions s JOIN edgeagent_stream_admissions a USING(session_id) WHERE a.account_id=$1 AND s.active"#,account).fetch_one(&mut *tx).await.map_err(Error::storage)?;
        if active >= i64::from(policy.connections) {
            return Err(Error::new(K::Busy));
        }
        if usage.starts >= session.data().limits.starts.get()
            || policy.charged_starts >= policy.start_limit
        {
            return Err(Error::new(K::Exhausted));
        }
        if policy.last_start_ms.is_some_and(|last| {
            last.checked_add(policy.spacing_ms)
                .is_none_or(|next| at.unix_millis() < next)
        }) {
            return Err(Error::new(K::RateLimited));
        }
        let generation = policy
            .generation
            .checked_add(1)
            .filter(|g| *g > 0)
            .ok_or_else(|| Error::new(K::Exhausted))?;
        let starts = i64::from(usage.starts) + 1;
        sqlx::query!("UPDATE edgeagent_stream_accounts SET charged_starts=charged_starts+1,generation=$2,last_start_ms=$3 WHERE account_id=$1",account,generation,at.unix_millis()).execute(&mut *tx).await.map_err(Error::storage)?;
        sqlx::query!("UPDATE edgeagent_stream_sessions SET starts=$2,generation=$3,active=TRUE,generation_start=frames+1 WHERE session_id=$1",session.data().id.as_str(),starts,generation).execute(&mut *tx).await.map_err(Error::storage)?;
        session.recheck(catalogs, now(&mut tx).await?, cancellation)?;
        tx.commit().await.map_err(Error::storage)?;
        StreamingConnection::from_committed(
            session,
            u64::try_from(generation).map_err(|_| invalid())?,
            StreamingUsage {
                starts: usage.starts + 1,
                ..usage
            },
        )
    }
    async fn charge(
        &self,
        session: &StreamingSession,
        connection: &StreamingConnection,
        catalogs: AuthorityCatalogs<'_>,
        cancellation: &Cancellation,
        arrival: StreamingArrival,
    ) -> Result<StreamingFrame> {
        if !connection.matches(session) {
            return Err(conflict());
        }
        let mut tx = self.ledger.transaction().await?;
        let policy = lock_account(&mut tx, &key(session.account())).await?;
        worker(&mut tx, session).await?;
        recovery::enforce(&mut tx, session, self.fence.as_ref(), false).await?;
        let state = state(&mut tx, &session.data().id).await?;
        let at = now(&mut tx).await?;
        validate_policy(&policy, session, at)?;
        session.recheck(catalogs, at, cancellation)?;
        if !state.active
            || u64::try_from(state.generation).map_err(|_| invalid())? != connection.generation()
        {
            return Err(denied());
        }
        let result =
            session
                .data()
                .limits
                .charge(state.status()?.usage, arrival.ordinal, arrival.bytes)?;
        if !result.repeated {
            let frames = i64::try_from(result.usage.frames).map_err(|_| invalid())?;
            let bytes = i64::try_from(result.usage.bytes).map_err(|_| invalid())?;
            sqlx::query!("UPDATE edgeagent_stream_sessions SET frames=$2,bytes=$3,last_frame_bytes=$4,stopped=$5 WHERE session_id=$1",session.data().id.as_str(),frames,bytes,i64::from(arrival.bytes),result.usage.stopped).execute(&mut *tx).await.map_err(Error::storage)?;
        }
        session.recheck(catalogs, now(&mut tx).await?, cancellation)?;
        tx.commit().await.map_err(Error::storage)?;
        Ok(result)
    }
}
