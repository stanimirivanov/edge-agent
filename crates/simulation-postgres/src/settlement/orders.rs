//! Terminal order journals and hold release share the settlement transaction.
use super::SettlementContext;
use crate::{hex, invariant, permanent, signed, storage, unsigned};
use edgeagent_market_data_domain::PreciseInstant;
use edgeagent_messaging::HandlerFailure;
use edgeagent_simulation_domain::{Run, execution::FillAccounting, orders::*};
use sqlx::{Postgres, Transaction};

pub(super) async fn expire(
    context: &SettlementContext<'_, '_, '_, '_>,
    tx: &mut Transaction<'_, Postgres>,
    instrument: &str,
    at: PreciseInstant,
    run: &mut Run<'_>,
) -> Result<Vec<(String, u64)>, HandlerFailure> {
    let owner = context.execution.admission;
    let account = owner.account();
    let mut expired = Vec::new();
    for quote in owner
        .quotes
        .iter()
        .filter(|q| format!("{:032x}", q.order().instrument().id().get()) == instrument)
    {
        let definition = quote.order();
        let order_id = hex(&definition.scope().identity().3.bytes());
        let initial = sqlx::query!(
            "SELECT admission_journal FROM simulation_order WHERE account=$1 AND order_id=$2",
            account,
            order_id
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(storage)?
        .admission_journal
        .ok_or_else(|| permanent("settlement_legacy_order"))?;
        let stored=sqlx::query!("SELECT journal,exact_cents,filled_units,gross_cents,fees_cents FROM simulation_execution_order WHERE account=$1 AND order_id=$2",account,order_id).fetch_optional(&mut **tx).await.map_err(storage)?;
        let bytes = stored
            .as_ref()
            .map_or(initial.as_slice(), |s| s.journal.as_slice());
        let mut order =
            Order::restore_journal(definition.clone(), 4096, quote.fingerprint(), bytes)
                .map_err(|_| invariant())?;
        let totals = FillAccounting::retained_totals(&order, context.execution.profile)
            .map_err(|_| invariant())?;
        let cached = if let Some(s) = &stored {
            (
                s.exact_cents.parse::<u128>().map_err(|_| invariant())?,
                unsigned(s.filled_units)?,
                unsigned(s.gross_cents)?,
                unsigned(s.fees_cents)?,
            )
        } else {
            (0, 0, 0, 0)
        };
        if cached != totals {
            return Err(invariant());
        }
        let held=sqlx::query!("SELECT strategy,instrument,unit,cash_cents,quantity FROM simulation_reservation WHERE account=$1 AND order_id=$2",account,order_id).fetch_optional(&mut **tx).await.map_err(storage)?;
        let terminal = matches!(
            order.state().0,
            OrderStatus::Filled
                | OrderStatus::Cancelled
                | OrderStatus::Expired
                | OrderStatus::Rejected
        );
        if terminal == held.is_some() {
            return Err(invariant());
        }
        if let Some(held) = held {
            let (spent, remaining_units) = match definition.terms().0 {
                OrderSide::BuyToOpen => (totals.2.checked_add(totals.3).ok_or_else(invariant)?, 0),
                OrderSide::SellToClose => (
                    totals.3,
                    definition
                        .terms()
                        .2
                        .parts()
                        .1
                        .checked_sub(totals.1)
                        .ok_or_else(invariant)?,
                ),
                _ => return Err(invariant()),
            };
            let remaining_cash = quote
                .cash()
                .map_err(|_| invariant())?
                .get()
                .checked_sub(spent)
                .ok_or_else(invariant)?;
            if unsigned(held.cash_cents)? != remaining_cash
                || unsigned(held.quantity)? != remaining_units
            {
                return Err(invariant());
            }
            if held.strategy != format!("{:032x}", definition.scope().identity().2.get())
                || held.instrument != instrument
                || held.unit != "contracts"
                || at < definition.lifetime().bounds().1
            {
                return Err(invariant());
            }
            let event = OrderEvent::new(
                OrderEventId::new(run.next_id().map_err(|_| invariant())?),
                definition.scope().identity().3,
                order.state().1 + 1,
                at,
                OrderEventKind::Expired,
            )
            .map_err(|_| invariant())?;
            order.apply(event).map_err(|_| invariant())?;
            let journal = order.journal_bytes(quote.fingerprint());
            let exact = totals.0.to_string();
            let units = signed(totals.1)?;
            let gross = signed(totals.2)?;
            let fees = signed(totals.3)?;
            sqlx::query!("INSERT INTO simulation_execution_order(account,order_id,journal,exact_cents,filled_units,gross_cents,fees_cents) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(account,order_id) DO UPDATE SET journal=EXCLUDED.journal,exact_cents=EXCLUDED.exact_cents,filled_units=EXCLUDED.filled_units,gross_cents=EXCLUDED.gross_cents,fees_cents=EXCLUDED.fees_cents",account,order_id,journal,exact,units,gross,fees).execute(&mut **tx).await.map_err(storage)?;
            sqlx::query!(
                "DELETE FROM simulation_reservation WHERE account=$1 AND order_id=$2",
                account,
                order_id
            )
            .execute(&mut **tx)
            .await
            .map_err(storage)?;
            expired.push((order_id, order.state().1));
        }
    }
    expired.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(expired)
}
