use super::SettlementContext;
use crate::{invariant, storage};
use edgeagent_messaging::HandlerFailure;
use sqlx::{Connection, PgConnection};
/// Provision an immutable postlude on a newly funded account, after execution
/// provisioning and before any admission. Runtime has SELECT only on this table.
/// # Errors
/// Reject reused/changed plans, nonzero account inputs or a foreign execution plan.
pub async fn provision_settlement(
    connection: &mut PgConnection,
    context: &SettlementContext<'_, '_, '_, '_>,
) -> Result<(), HandlerFailure> {
    let mut tx = connection.begin().await.map_err(storage)?;
    let account = context.execution.admission.account();
    let root = sqlx::query!(
        "SELECT configuration,input_revision FROM simulation_account WHERE account=$1 FOR UPDATE",
        account
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(storage)?;
    let execution = sqlx::query!(
        "SELECT binding FROM simulation_execution_config WHERE account=$1",
        account
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(storage)?;
    if root.input_revision != 0
        || root.configuration != context.execution.admission.configuration.bytes()
        || execution.binding != context.execution.binding.bytes()
    {
        return Err(invariant());
    }
    let binding = context.binding.bytes();
    sqlx::query!(
        "INSERT INTO simulation_settlement_config(account,binding,canonical) VALUES($1,$2,$3)",
        account,
        &binding[..],
        context.canonical
    )
    .execute(&mut *tx)
    .await
    .map_err(storage)?;
    tx.commit().await.map_err(storage)
}
