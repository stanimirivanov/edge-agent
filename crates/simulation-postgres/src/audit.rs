//! Owner-only capture/export. Call capture under the already acquired root lock.
//! No peer reader is granted table access; composition exports checked records.
use crate::{AdmissionContext, hex, invariant, permanent, signed, storage, unsigned};
use edgeagent_contracts::audit::{
    AccountAuditFrame, AccountAuditState, AuditPosition, AuditValuation,
};
use edgeagent_messaging::HandlerFailure;
use edgeagent_simulation_domain::{
    ArtifactDigest,
    risk::{RiskReason, RiskState},
};
use sqlx::{PgConnection, Postgres, Transaction};

mod orders;

/// Bounded owner export page; high-water is not a timestamp/freshness guarantee.
pub struct AuditPage {
    /// Contiguous retained records after the requested exclusive cursor.
    pub frames: Vec<AccountAuditFrame>,
    /// Authoritative account revision observed before the immutable page read.
    pub owner_revision: u64,
}
impl std::fmt::Debug for AuditPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditPage")
            .field("records", &self.frames.len())
            .field("owner_revision", &self.owner_revision)
            .finish_non_exhaustive()
    }
}
/// Export through ExecutionSimulator's verified-principal composition, never a
/// peer database login. Limit 1..=8, each record <=1 MiB. Start with `after=None`.
/// Reuse the same account/run/configuration and fetch until the high-water mark.
///
/// # Errors
/// Reject unauthorized/configuration mismatch, missing history, invalid cursor,
/// malformed retained content or storage failure. No history is synthesized.
pub async fn export_audit(
    connection: &mut PgConnection,
    context: &AdmissionContext<'_>,
    after: Option<u64>,
    limit: usize,
) -> Result<AuditPage, HandlerFailure> {
    if !(1..=8).contains(&limit) {
        return Err(permanent("audit_capacity"));
    }
    let account = context.account();
    let root = sqlx::query!(
        "SELECT a.revision, a.configuration
         FROM simulation_account a
         JOIN simulation_account_grant g USING (account)
         WHERE a.account = $1 AND g.principal = $2",
        account,
        context.principal
    )
    .fetch_optional(&mut *connection)
    .await
    .map_err(storage)?
    .ok_or_else(|| permanent("audit_unauthorized"))?;
    if root.configuration != context.configuration.bytes() {
        return Err(permanent("audit_scope"));
    }
    let water = unsigned(root.revision)?;
    if after.is_some_and(|n| n > water) {
        return Err(permanent("audit_cursor"));
    }
    let cursor = after.map(signed).transpose()?.unwrap_or(-1);
    let limit = signed(limit as u64)?;
    let rows = sqlx::query!(
        "SELECT revision, payload FROM simulation_account_audit
         WHERE account = $1 AND revision > $2 AND revision <= $3
         ORDER BY revision LIMIT $4",
        account,
        cursor,
        root.revision,
        limit
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(storage)?;
    let first_revision = after.map_or(0, |n| n + 1);
    let mut frames = Vec::with_capacity(rows.len());
    for (expected, row) in (first_revision..).zip(rows) {
        let frame = AccountAuditFrame::decode(&row.payload).map_err(|_| invariant())?;
        if unsigned(row.revision)? != expected
            || frame.state().revision != expected
            || frame.state().account != account
            || frame.state().run != hex(&context.manifest.digest().bytes())
            || frame.state().configuration != hex(&context.configuration.bytes())
        {
            return Err(invariant());
        }
        frames.push(frame);
    }
    if frames.is_empty() && after != Some(water) {
        return Err(permanent("audit_history_missing"));
    }
    Ok(AuditPage {
        frames,
        owner_revision: water,
    })
}
pub(crate) async fn require_unstarted(
    tx: &mut Transaction<'_, Postgres>,
    account: &str,
) -> Result<(), HandlerFailure> {
    let row = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM simulation_account_audit WHERE account=$1) AS \"started!\"",
        account
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage)?;
    if row.started {
        return Err(permanent("audit_bootstrap_closed"));
    }
    Ok(())
}
pub(crate) async fn ensure(
    tx: &mut Transaction<'_, Postgres>,
    context: &AdmissionContext<'_>,
    revision: i64,
) -> Result<(), HandlerFailure> {
    let account = context.account();
    let exists = sqlx::query!(
        "SELECT revision FROM simulation_account_audit WHERE account=$1 AND revision=$2",
        account,
        revision
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage)?;
    if exists.is_none() {
        if revision != 0 {
            return Err(permanent("audit_history_missing"));
        }
        capture(tx, context).await?;
    }
    Ok(())
}
pub(crate) async fn capture(
    tx: &mut Transaction<'_, Postgres>,
    context: &AdmissionContext<'_>,
) -> Result<(), HandlerFailure> {
    let account = context.account();
    let root = sqlx::query!(
        r#"SELECT a.revision, a.input_revision, a.cash_cents, a.risk_checkpoint,
                  e.binding AS "execution?", s.binding AS "settlement?"
           FROM simulation_account a
           LEFT JOIN simulation_execution_config e USING (account)
           LEFT JOIN simulation_settlement_config s USING (account)
           WHERE a.account = $1"#,
        account
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage)?;
    let revision = unsigned(root.revision)?;
    let previous = if revision == 0 {
        None
    } else {
        let prior = root.revision - 1;
        let row = sqlx::query!(
            "SELECT payload FROM simulation_account_audit WHERE account=$1 AND revision=$2",
            account,
            prior
        )
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage)?
        .ok_or_else(|| permanent("audit_history_missing"))?;
        let frame = AccountAuditFrame::decode(&row.payload).map_err(|_| invariant())?;
        if frame.state().revision != revision - 1
            || frame.state().account != account
            || frame.state().configuration != hex(&context.configuration.bytes())
            || frame.state().run != hex(&context.manifest.digest().bytes())
        {
            return Err(invariant());
        }
        Some(hex(&ArtifactDigest::of(
            &frame.encode().map_err(|_| invariant())?,
        )
        .bytes()))
    };
    let risk = RiskState::restore(
        context.account,
        context.policy.clone(),
        context.session,
        &root.risk_checkpoint,
    )
    .map_err(|_| invariant())?;
    let rows = sqlx::query!(
        r#"SELECT strategy, gross_cents, loss_cents, valued_seconds, valued_nanos, artifact
           FROM simulation_valuation WHERE account = $1
           ORDER BY strategy COLLATE "C" LIMIT 33"#,
        account
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(storage)?;
    let mut valuations = Vec::with_capacity(rows.len());
    for r in rows {
        valuations.push(AuditValuation {
            strategy: r.strategy,
            gross_cents: unsigned(r.gross_cents)?,
            loss_cents: unsigned(r.loss_cents)?,
            valued_seconds: r.valued_seconds,
            valued_nanos: u32::try_from(r.valued_nanos).map_err(|_| invariant())?,
            artifact: hex(&r.artifact),
        });
    }
    let rows = sqlx::query!(
        r#"SELECT i.strategy, i.instrument, i.unit, i.quantity, c.cost_cents AS "cost_cents?"
           FROM simulation_inventory i
           LEFT JOIN simulation_position_cost c USING (account, strategy, instrument, unit)
           WHERE i.account = $1
           ORDER BY i.strategy COLLATE "C", i.instrument COLLATE "C", i.unit COLLATE "C"
           LIMIT 4097"#,
        account
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(storage)?;
    let mut positions = Vec::with_capacity(rows.len());
    for r in rows {
        positions.push(AuditPosition {
            strategy: r.strategy,
            instrument: r.instrument,
            unit: r.unit,
            quantity: unsigned(r.quantity)?,
            cost_cents: r.cost_cents.map(unsigned).transpose()?,
        });
    }
    let ledger = sqlx::query!(
        r#"SELECT COALESCE(SUM(cash_delta), 0)::BIGINT AS "cash!",
                  COALESCE(SUM(position_delta), 0)::BIGINT AS "cost!",
                  COALESCE(SUM(realized_delta), 0)::BIGINT AS "realized!",
                  COALESCE(SUM(fee_delta), 0)::BIGINT AS "fees!"
           FROM (
               SELECT cash_delta, position_delta, realized_delta, fee_delta
               FROM simulation_execution_receipt WHERE account = $1
               UNION ALL
               SELECT cash_delta, position_delta, realized_delta, fee_delta
               FROM simulation_settlement_receipt WHERE account = $1
           ) effects"#,
        account
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage)?;
    let state = AccountAuditState {
        account: account.clone(),
        run: hex(&context.manifest.digest().bytes()),
        configuration: hex(&context.configuration.bytes()),
        execution_plan: root.execution.as_deref().map(hex),
        settlement_plan: root.settlement.as_deref().map(hex),
        revision,
        input_revision: unsigned(root.input_revision)?,
        cash_cents: unsigned(root.cash_cents)?,
        risk_policy: hex(&context.policy.fingerprint().bytes()),
        risk_revision: risk.status().0,
        risk_breach: risk.status().1.map(reason).transpose()?,
        risk_checkpoint: hex(&root.risk_checkpoint),
        orders: orders::read(tx, context).await?,
        positions,
        valuations,
        postings: [ledger.cash, ledger.cost, ledger.realized, ledger.fees],
    };
    let frame = AccountAuditFrame::new(state, previous).map_err(|_| invariant())?;
    let payload = frame.encode().map_err(|_| invariant())?;
    sqlx::query!(
        "INSERT INTO simulation_account_audit(account,revision,payload) VALUES($1,$2,$3)",
        account,
        root.revision,
        payload
    )
    .execute(&mut **tx)
    .await
    .map_err(storage)?;
    Ok(())
}
fn reason(reason: RiskReason) -> Result<String, HandlerFailure> {
    let (code, strategy) = match reason {
        RiskReason::StrategyGross(id) => ("strategy_gross", Some(id)),
        RiskReason::StrategyLoss(id) => ("strategy_loss", Some(id)),
        RiskReason::StrategyOrders(id) => ("strategy_orders", Some(id)),
        r => (crate::processing::reason_code(r)?, None),
    };
    Ok(strategy.map_or_else(|| code.into(), |id| format!("{code}:{:032x}", id.get())))
}
