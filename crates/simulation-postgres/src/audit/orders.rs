use super::*;
use edgeagent_contracts::audit::AuditOrder;
use edgeagent_simulation_domain::orders::{Order, OrderSide, OrderStatus};

// Replay each original bound journal, rather than interpreting command statuses
// such as `already_terminal`. One journal at a time bounds temporary memory.
pub(super) async fn read(
    tx: &mut Transaction<'_, Postgres>,
    context: &AdmissionContext<'_>,
) -> Result<Vec<AuditOrder>, HandlerFailure> {
    let account = context.account();
    let rows = sqlx::query!(
        r#"SELECT order_id FROM simulation_order WHERE account = $1
           ORDER BY order_id COLLATE "C" LIMIT 4097"#,
        account
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(storage)?;
    if rows.len() > 4096 {
        return Err(permanent("audit_capacity"));
    }
    let mut orders = Vec::with_capacity(rows.len());
    let catalog = context
        .quotes
        .iter()
        .map(|quote| (hex(&quote.order().scope().identity().3.bytes()), quote))
        .collect::<std::collections::BTreeMap<_, _>>();
    for row in rows {
        let id = row.order_id;
        let quote = catalog.get(&id).ok_or_else(invariant)?;
        let row = sqlx::query!(
            r#"SELECT COALESCE(e.journal, o.admission_journal) AS "journal?",
                      r.cash_cents AS "cash?", r.quantity AS "quantity?"
               FROM simulation_order o
               LEFT JOIN simulation_execution_order e USING (account, order_id)
               LEFT JOIN simulation_reservation r USING (account, order_id)
               WHERE o.account = $1 AND o.order_id = $2"#,
            account,
            id
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(storage)?;
        let journal = row.journal.ok_or_else(|| permanent("audit_legacy_order"))?;
        let order =
            Order::restore_journal(quote.order().clone(), 4096, quote.fingerprint(), &journal)
                .map_err(|_| invariant())?;
        let status = match order.state().0 {
            OrderStatus::Working => "working",
            OrderStatus::PartiallyFilled => "partially_filled",
            OrderStatus::CancelPending => "cancel_pending",
            OrderStatus::Filled => "filled",
            OrderStatus::Cancelled => "cancelled",
            OrderStatus::Expired => "expired",
            OrderStatus::Rejected => "rejected",
            _ => return Err(invariant()),
        };
        let terminal = matches!(status, "filled" | "cancelled" | "expired" | "rejected");
        if terminal == row.cash.is_some() || row.cash.is_some() != row.quantity.is_some() {
            return Err(invariant());
        }
        let d = order.definition();
        orders.push(AuditOrder {
            order: id,
            strategy: format!("{:032x}", d.scope().identity().2.get()),
            instrument: format!("{:032x}", d.instrument().id().get()),
            unit: crate::provision::unit(d.terms().2.parts().0)?.into(),
            side: match d.terms().0 {
                OrderSide::BuyToOpen => "buy_to_open",
                OrderSide::SellToClose => "sell_to_close",
                _ => return Err(invariant()),
            }
            .into(),
            revision: order.state().1,
            status: status.into(),
            quantity: d.terms().2.parts().1,
            filled: order.quantities().0.map_or(0, |q| q.parts().1),
            reserved_cash_cents: unsigned(row.cash.unwrap_or(0))?,
            reserved_units: unsigned(row.quantity.unwrap_or(0))?,
        });
    }
    Ok(orders)
}
