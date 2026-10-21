//! Transaction-local root/authorization/configuration/receipt fence. Never commits.
use super::SettlementContext;
use crate::{hex, invariant, permanent, signed, storage, unsigned};
use edgeagent_contracts::{
    Component, MessageEnvelope,
    settlement::{DEFINITIONS, SettleDryRunAccount},
};
use edgeagent_messaging::{FailureCode, HandlerFailure};
use sqlx::{Postgres, Transaction};
pub(super) struct Root {
    pub revision: i64,
    pub input_revision: i64,
    pub cash_cents: i64,
    pub risk_checkpoint: Vec<u8>,
    pub run_checkpoint: Vec<u8>,
}
pub(super) struct Fenced {
    pub root: Root,
    pub request: SettleDryRunAccount,
    pub command: Vec<u8>,
}
pub(super) async fn acquire(
    context: &SettlementContext<'_, '_, '_, '_>,
    tx: &mut Transaction<'_, Postgres>,
    envelope: &MessageEnvelope,
) -> Result<Option<Fenced>, HandlerFailure> {
    DEFINITIONS[0]
        .validate_envelope(envelope)
        .map_err(|_| permanent("settlement_contract"))?;
    let request: SettleDryRunAccount = envelope
        .payload()
        .map_err(|_| permanent("settlement_contract"))?;
    let (account, plan, sequence, expected) = request.position();
    let owner = context.execution.admission;
    if account != owner.account()
        || plan != hex(&context.binding.bytes())
        || envelope.source() != Component::Gateway.source_uri()
        || envelope.partition_key() != Some(format!("account/{account}").as_str())
    {
        return Err(permanent("settlement_scope"));
    }
    let lock = format!("{}ms", owner.limits.lock_ms);
    let statement = format!("{}ms", owner.limits.statement_ms);
    let settings=sqlx::query!(r#"SELECT current_setting('transaction_isolation') AS "isolation!", set_config('lock_timeout',$1,true) AS "lock!", set_config('statement_timeout',$2,true) AS "statement!""#,lock,statement).fetch_one(&mut **tx).await.map_err(storage)?;
    if settings.isolation != "read committed" {
        return Err(permanent("settlement_isolation"));
    }
    let root=sqlx::query!("SELECT configuration,revision,input_revision,cash_cents,risk_checkpoint,run_checkpoint FROM simulation_account WHERE account=$1 FOR UPDATE",account).fetch_one(&mut **tx).await.map_err(storage)?;
    let grant = sqlx::query!(
        "SELECT principal FROM simulation_account_grant WHERE account=$1 AND principal=$2",
        account,
        owner.principal
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage)?;
    if grant.is_none() {
        return Err(permanent("settlement_unauthorized"));
    }
    let profile = context.execution.profile.identity().1.bytes();
    let config=sqlx::query!(r#"SELECT s.binding,e.binding AS execution,sha256(s.canonical)=s.binding AND sha256(e.canonical)=e.binding AND sha256(e.profile)=$2 AS "valid!" FROM simulation_settlement_config s JOIN simulation_execution_config e USING(account) WHERE s.account=$1"#,account,&profile[..]).fetch_one(&mut **tx).await.map_err(storage)?;
    if root.configuration != owner.configuration.bytes()
        || config.binding != context.binding.bytes()
        || config.execution != context.execution.binding.bytes()
        || !config.valid
    {
        return Err(permanent("settlement_configuration"));
    }
    let command = serde_json::to_vec(&request).map_err(|_| invariant())?;
    let seq = signed(sequence)?;
    if let Some(receipt) = sqlx::query!(
        "SELECT request FROM simulation_settlement_receipt WHERE account=$1 AND sequence=$2",
        account,
        seq
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage)?
    {
        return if receipt.request == command {
            Ok(None)
        } else {
            Err(permanent("settlement_identity"))
        };
    }
    let next = unsigned(root.input_revision)?
        .checked_add(1)
        .ok_or_else(invariant)?;
    if sequence > next {
        return Err(HandlerFailure::transient(FailureCode::from_static(
            "settlement_input_gap",
        )));
    }
    if sequence != next || expected != unsigned(root.revision)? {
        return Err(permanent("settlement_revision"));
    }
    Ok(Some(Fenced {
        request,
        command,
        root: Root {
            revision: root.revision,
            input_revision: root.input_revision,
            cash_cents: root.cash_cents,
            risk_checkpoint: root.risk_checkpoint,
            run_checkpoint: root.run_checkpoint,
        },
    }))
}
