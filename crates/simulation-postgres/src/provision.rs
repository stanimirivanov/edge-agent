use super::{AdmissionContext, hex, invariant, signed, storage};
use edgeagent_market_data_domain::{InstrumentId, PreciseInstant};
use edgeagent_messaging::HandlerFailure;
use edgeagent_simulation_domain::{
    ArtifactDigest,
    orders::QuantityUnit,
    risk::{RiskSnapshot, RiskState, RiskStrategyId, RiskUsage, StrategyValuation, UsdCents},
};
use sqlx::{Connection, PgConnection, Postgres, Transaction};

/// Explicit service-authored synthetic position valuation, not a caller claim.
#[derive(Clone, Copy, Debug)]
pub struct InitialValuation {
    /// Stable strategy allocation identity.
    pub strategy: RiskStrategyId,
    /// Gross value of all its existing positions, not including new reservations.
    pub position_gross: UsdCents,
    /// Nonnegative adverse current-session contribution; no profit netting.
    pub session_loss: UsdCents,
    /// Actual valuation instant, never refreshed by request receipt.
    pub valued: PreciseInstant,
    /// Verified retained synthetic valuation/baseline artifact.
    pub artifact: ArtifactDigest,
}
/// Service-authored long inventory allocation. No borrowing/shorting or implicit
/// transfers between strategies; unknown inventory is never treated as available.
#[derive(Clone, Copy)]
pub struct InitialInventory {
    /// Owning strategy within the account.
    pub strategy: RiskStrategyId,
    /// Stable instrument, not a provider symbol or proxy.
    pub instrument: InstrumentId,
    /// Shares versus option contracts.
    pub unit: QuantityUnit,
    /// Existing whole long quantity; zero is explicit.
    pub quantity: u64,
}
impl std::fmt::Debug for InitialInventory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InitialInventory").finish_non_exhaustive()
    }
}
/// Atomically provision a NEW private account and its immutable synthetic catalog.
/// No upsert, cash top-up, grant reset or reinitialization of existing accounts.
/// Apply migrations separately with a migration-owner role before calling this.
///
/// # Errors
/// Reject duplicate/invalid bootstrap data, incomplete/future valuations, storage
/// failures or an existing account. Failure rolls the entire bootstrap back.
pub async fn provision(
    connection: &mut PgConnection,
    context: &AdmissionContext<'_>,
    cash: UsdCents,
    valuations: &[InitialValuation],
    inventory: &[InitialInventory],
) -> Result<(), HandlerFailure> {
    if valuations.is_empty() || valuations.len() > 32 || inventory.len() > 4096 {
        return Err(invariant());
    }
    let mut transaction = connection.begin().await.map_err(storage)?;
    let result = insert(&mut transaction, context, cash, valuations, inventory).await;
    match result {
        Ok(()) => transaction.commit().await.map_err(storage),
        Err(error) => {
            transaction.rollback().await.map_err(storage)?;
            Err(error)
        }
    }
}
async fn insert(
    transaction: &mut Transaction<'_, Postgres>,
    context: &AdmissionContext<'_>,
    cash: UsdCents,
    valuations: &[InitialValuation],
    inventory: &[InitialInventory],
) -> Result<(), HandlerFailure> {
    let at = context
        .manifest
        .inputs()
        .first()
        .ok_or_else(invariant)?
        .available_at();
    let rows = valuations
        .iter()
        .map(|row| {
            Ok(StrategyValuation::new(
                row.strategy,
                RiskUsage::new(row.position_gross, UsdCents::new(0), row.session_loss, 0)
                    .map_err(|_| invariant())?,
                row.valued,
            ))
        })
        .collect::<Result<Vec<_>, HandlerFailure>>()?;
    let snapshot =
        RiskSnapshot::new(context.account, context.session, &rows).map_err(|_| invariant())?;
    let mut risk = RiskState::new(context.account, context.policy.clone(), context.session, at)
        .map_err(|_| invariant())?;
    risk.observe(1, &snapshot, at).map_err(|_| invariant())?;
    let account = context.account();
    let configuration = context.configuration.bytes();
    let cash = signed(cash.get())?;
    let risk_checkpoint = risk.checkpoint();
    let run_checkpoint = context.initial_run.clone();
    sqlx::query!("INSERT INTO simulation_account (account,configuration,cash_cents,risk_checkpoint,run_checkpoint) VALUES ($1,$2,$3,$4,$5)",
        account,&configuration[..],cash,risk_checkpoint,run_checkpoint).execute(&mut **transaction).await.map_err(storage)?;
    sqlx::query!(
        "INSERT INTO simulation_account_grant (account,principal) VALUES ($1,$2)",
        account,
        context.principal
    )
    .execute(&mut **transaction)
    .await
    .map_err(storage)?;
    for row in valuations {
        let strategy = format!("{:032x}", row.strategy.get());
        let gross = signed(row.position_gross.get())?;
        let loss = signed(row.session_loss.get())?;
        let (seconds, nanos) = row.valued.unix_parts();
        let nanos = signed(nanos)?;
        let artifact = row.artifact.bytes();
        sqlx::query!("INSERT INTO simulation_valuation (account,strategy,gross_cents,loss_cents,valued_seconds,valued_nanos,artifact) VALUES ($1,$2,$3,$4,$5,$6,$7)",
            account,strategy,gross,loss,seconds,nanos,&artifact[..]).execute(&mut **transaction).await.map_err(storage)?;
    }
    for row in inventory {
        let strategy = format!("{:032x}", row.strategy.get());
        let instrument = format!("{:032x}", row.instrument.get());
        let unit = unit(row.unit)?;
        let quantity = signed(row.quantity)?;
        sqlx::query!("INSERT INTO simulation_inventory (account,strategy,instrument,unit,quantity) VALUES ($1,$2,$3,$4,$5)",
            account,strategy,instrument,unit,quantity).execute(&mut **transaction).await.map_err(storage)?;
    }
    for quote in &context.quotes {
        let fingerprint = hex(&quote.fingerprint().bytes());
        let sequence = signed(quote.position().0)?;
        sqlx::query!("INSERT INTO simulation_quote (account,fingerprint,input_revision,canonical,profile) VALUES ($1,$2,$3,$4,'synthetic-reservation-v1')",
            account,fingerprint,sequence,quote.canonical_bytes()).execute(&mut **transaction).await.map_err(storage)?;
    }
    Ok(())
}
pub(crate) fn unit(unit: QuantityUnit) -> Result<&'static str, HandlerFailure> {
    match unit {
        QuantityUnit::Shares => Ok("shares"),
        QuantityUnit::Contracts => Ok("contracts"),
        _ => Err(invariant()),
    }
}
