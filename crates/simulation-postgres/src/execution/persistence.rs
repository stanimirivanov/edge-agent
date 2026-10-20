//! Financial mutations run only after the caller owns the account root lock.
use super::ExecutionContext;
use crate::{invariant, signed, storage, unsigned};
use edgeagent_market_data_domain::PreciseInstant;
use edgeagent_messaging::HandlerFailure;
use edgeagent_simulation_domain::{execution::PositionCost, risk::*};
use sqlx::{Postgres, Transaction};

pub(super) async fn position(
    tx: &mut Transaction<'_, Postgres>,
    account: &str,
    key: (&str, &str, &str),
    position: PositionCost,
    postings: [i128; 4],
) -> Result<(), HandlerFailure> {
    let (strategy, instrument, unit) = key;
    let quantity = signed(position.values().0)?;
    let cost = signed(position.values().1)?;
    let row=sqlx::query!("UPDATE simulation_inventory SET quantity=$5 WHERE account=$1 AND strategy=$2 AND instrument=$3 AND unit=$4",account,strategy,instrument,unit,quantity).execute(&mut **tx).await.map_err(storage)?;
    if row.rows_affected() != 1 {
        return Err(invariant());
    }
    let row=sqlx::query!("UPDATE simulation_position_cost SET cost_cents=$5 WHERE account=$1 AND strategy=$2 AND instrument=$3 AND unit=$4",account,strategy,instrument,unit,cost).execute(&mut **tx).await.map_err(storage)?;
    if row.rows_affected() != 1 {
        return Err(invariant());
    }
    // Conservative risk carry-forward, NOT a fresh market mark. Never reduce a
    // gross limit on sale or refresh old valuation timestamps on execution. P08
    // stale-mark denial and sticky breach policy remain intact. Book P/L and fees
    // are exact and independently increase adverse session contribution.
    let gross = i64::try_from(postings[1].max(0)).map_err(|_| invariant())?;
    let loss = i64::try_from(
        postings[2]
            .max(0)
            .checked_add(postings[3])
            .ok_or_else(invariant)?,
    )
    .map_err(|_| invariant())?;
    let changed=sqlx::query!("UPDATE simulation_valuation SET gross_cents=gross_cents+$3,loss_cents=loss_cents+$4 WHERE account=$1 AND strategy=$2",account,strategy,gross,loss).execute(&mut **tx).await.map_err(storage)?;
    if changed.rows_affected() != 1 {
        return Err(invariant());
    }
    Ok(())
}
pub(super) async fn risk(
    tx: &mut Transaction<'_, Postgres>,
    context: &ExecutionContext<'_, '_, '_>,
    checkpoint: &[u8],
    at: PreciseInstant,
) -> Result<Vec<u8>, HandlerFailure> {
    let owner = context.admission;
    let account = owner.account();
    let rows=sqlx::query!(r#"SELECT v.strategy,v.gross_cents,v.loss_cents,v.valued_seconds,v.valued_nanos,v.artifact,COALESCE(r.cash,0)::BIGINT AS "cash!",COALESCE(r.orders,0)::BIGINT AS "orders!" FROM simulation_valuation v LEFT JOIN (SELECT strategy,SUM(cash_cents)::BIGINT AS cash,COUNT(*)::BIGINT AS orders FROM simulation_reservation WHERE account=$1 GROUP BY strategy) r USING(strategy) WHERE v.account=$1 ORDER BY v.strategy LIMIT 33"#,account).fetch_all(&mut **tx).await.map_err(storage)?;
    if rows.is_empty() || rows.len() > 32 {
        return Err(invariant());
    }
    let rows = rows
        .iter()
        .map(|r| {
            if r.artifact.len() != 32 {
                return Err(invariant());
            }
            let strategy = RiskStrategyId::new(
                u128::from_str_radix(&r.strategy, 16).map_err(|_| invariant())?,
            )
            .map_err(|_| invariant())?;
            let valued = PreciseInstant::from_unix_parts(
                r.valued_seconds,
                u32::try_from(r.valued_nanos).map_err(|_| invariant())?,
            )
            .map_err(|_| invariant())?;
            let usage = RiskUsage::new(
                UsdCents::new(unsigned(r.gross_cents)?),
                UsdCents::new(unsigned(r.cash)?),
                UsdCents::new(unsigned(r.loss_cents)?),
                unsigned(r.orders)?,
            )
            .map_err(|_| invariant())?;
            Ok(StrategyValuation::new(strategy, usage, valued))
        })
        .collect::<Result<Vec<_>, HandlerFailure>>()?;
    let snapshot =
        RiskSnapshot::new(owner.account, owner.session, &rows).map_err(|_| invariant())?;
    let mut risk = RiskState::restore(
        owner.account,
        owner.policy.clone(),
        owner.session,
        checkpoint,
    )
    .map_err(|_| invariant())?;
    risk.observe(risk.status().0, &snapshot, at)
        .map_err(|_| invariant())?;
    Ok(risk.checkpoint())
}
pub(super) async fn receipt(
    tx: &mut Transaction<'_, Postgres>,
    account: &str,
    sequence: i64,
    command: &[u8],
    outcome: &[u8],
    postings: [i128; 4],
) -> Result<(), HandlerFailure> {
    let [cash, position, realized, fee] = postings.map(i64::try_from);
    let cash = cash.map_err(|_| invariant())?;
    let position = position.map_err(|_| invariant())?;
    let realized = realized.map_err(|_| invariant())?;
    let fee = fee.map_err(|_| invariant())?;
    sqlx::query!("INSERT INTO simulation_execution_receipt(account,sequence,request,outcome,cash_delta,position_delta,realized_delta,fee_delta) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",account,sequence,command,outcome,cash,position,realized,fee).execute(&mut **tx).await.map_err(storage)?;
    Ok(())
}
