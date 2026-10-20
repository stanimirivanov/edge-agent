use super::ExecutionContext;
use crate::{invariant, signed, storage, unsigned};
use edgeagent_messaging::HandlerFailure;
use edgeagent_simulation_domain::execution::PositionCost;
use sqlx::{Connection, PgConnection};

/// Explicit initial book cost for every existing/target position. No cost is
/// guessed from a risk mark, quote or today's market. Keys are checked catalog
/// identities, not an instruction to transfer ownership or create holdings.
#[derive(Clone)]
pub struct InitialPositionCost {
    /// Fixed-width strategy identity.
    pub strategy: String,
    /// Fixed-width instrument identity.
    pub instrument: String,
    /// Exact `shares` or `contracts` unit.
    pub unit: String,
    /// Explicit current quantity and aggregate book cost.
    pub position: PositionCost,
}
impl std::fmt::Debug for InitialPositionCost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InitialPositionCost(..)")
    }
}
/// Bind execution once to a new or already admitted account, under the root lock.
/// Every original inventory and targeted product needs explicit book cost. The
/// provisioning role owns this operation; runtime workers cannot edit the plan.
///
/// # Errors
/// Reject missing/extra/duplicate positions, changed configuration/quantity or
/// repeated provisioning. Any failure rolls back the complete initialization.
pub async fn provision_execution(
    connection: &mut PgConnection,
    context: &ExecutionContext<'_, '_, '_>,
    costs: &[InitialPositionCost],
) -> Result<(), HandlerFailure> {
    let mut tx = connection.begin().await.map_err(storage)?;
    let result = async {
        if costs.is_empty() || costs.len()>4096 { return Err(invariant()); }
        let account = context.admission.account();
        let root = sqlx::query!("SELECT configuration FROM simulation_account WHERE account=$1 FOR UPDATE",account).fetch_one(&mut *tx).await.map_err(storage)?;
        if root.configuration != context.admission.configuration.bytes() { return Err(invariant()); }
        let rows = sqlx::query!("SELECT strategy,instrument,unit,quantity FROM simulation_inventory WHERE account=$1 ORDER BY strategy,instrument,unit LIMIT 4097",account).fetch_all(&mut *tx).await.map_err(storage)?;
        if rows.len()>4096 { return Err(invariant()); }
        let mut required = std::collections::BTreeMap::new();
        for row in rows { required.insert((row.strategy,row.instrument,row.unit), unsigned(row.quantity)?); }
        for quote in &context.admission.quotes {
            let order = quote.order();
            required.entry((format!("{:032x}", order.scope().identity().2.get()),format!("{:032x}",order.instrument().id().get()),crate::provision::unit(order.terms().2.parts().0)?.into())).or_insert(0);
        }
        if costs.len()!=required.len() { return Err(invariant()); }
        for cost in costs {
            let key = (cost.strategy.clone(),cost.instrument.clone(),cost.unit.clone());
            if required.remove(&key)!=Some(cost.position.values().0) { return Err(invariant()); }
            let (quantity, cents) = cost.position.values(); let quantity=signed(quantity)?; let cents=signed(cents)?;
            sqlx::query!("INSERT INTO simulation_inventory(account,strategy,instrument,unit,quantity) VALUES($1,$2,$3,$4,$5) ON CONFLICT(account,strategy,instrument,unit) DO NOTHING",account,cost.strategy,cost.instrument,cost.unit,quantity).execute(&mut *tx).await.map_err(storage)?;
            sqlx::query!("INSERT INTO simulation_position_cost(account,strategy,instrument,unit,cost_cents) VALUES($1,$2,$3,$4,$5)",account,cost.strategy,cost.instrument,cost.unit,cents).execute(&mut *tx).await.map_err(storage)?;
        }
        let binding = context.binding.bytes(); let slots=signed(context.steps.len() as u64)?;
        let profile=context.profile.canonical_bytes();
        sqlx::query!("INSERT INTO simulation_execution_config(account,binding,slots,canonical,profile) VALUES($1,$2,$3,$4,$5)",account,&binding[..],slots,context.canonical,profile).execute(&mut *tx).await.map_err(storage)?;
        Ok(())
    }.await;
    match result {
        Ok(()) => tx.commit().await.map_err(storage),
        Err(e) => {
            tx.rollback().await.map_err(storage)?;
            Err(e)
        }
    }
}
