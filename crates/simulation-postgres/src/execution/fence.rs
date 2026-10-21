//! One ownership/identity gate, in explicit order. No financial reads precede
//! the root lock, and committed semantic duplicates precede new revision checks.
use super::ExecutionContext;
use crate::{hex, invariant, permanent, signed, storage, unsigned};
use edgeagent_contracts::{
    Component, MessageEnvelope,
    execution::{DEFINITIONS, ExecuteDryRunStep},
};
use edgeagent_messaging::{FailureCode, HandlerFailure};
use sqlx::{Postgres, Transaction};

pub(super) struct AccountRoot {
    pub configuration: Vec<u8>,
    pub revision: i64,
    pub input_revision: i64,
    pub cash_cents: i64,
    pub risk_checkpoint: Vec<u8>,
    pub run_checkpoint: Vec<u8>,
}
pub(super) struct Fenced {
    pub root: AccountRoot,
    pub request: ExecuteDryRunStep,
    pub command: Vec<u8>,
}
pub(super) async fn acquire(
    context: &ExecutionContext<'_, '_, '_>,
    tx: &mut Transaction<'_, Postgres>,
    envelope: &MessageEnvelope,
) -> Result<Option<Fenced>, HandlerFailure> {
    DEFINITIONS[0]
        .validate_envelope(envelope)
        .map_err(|_| permanent("execution_contract"))?;
    let request: ExecuteDryRunStep = envelope
        .payload()
        .map_err(|_| permanent("execution_contract"))?;
    let (account, plan, sequence, expected) = request.position();
    if account != context.admission.account()
        || plan != hex(&context.binding.bytes())
        || envelope.partition_key() != Some(format!("account/{account}").as_str())
        || envelope.source() != Component::Gateway.source_uri()
    {
        return Err(permanent("execution_scope"));
    }
    let lock = format!("{}ms", context.admission.limits.lock_ms);
    let statement = format!("{}ms", context.admission.limits.statement_ms);
    let settings=sqlx::query!(r#"SELECT current_setting('transaction_isolation') AS "isolation!", set_config('lock_timeout',$1,true) AS "lock!", set_config('statement_timeout',$2,true) AS "statement!""#,lock,statement).fetch_one(&mut **tx).await.map_err(storage)?;
    if settings.isolation != "read committed" {
        return Err(permanent("execution_isolation"));
    }
    let root=sqlx::query_as!(AccountRoot,"SELECT configuration,revision,input_revision,cash_cents,risk_checkpoint,run_checkpoint FROM simulation_account WHERE account=$1 FOR UPDATE",account).fetch_one(&mut **tx).await.map_err(storage)?;
    let grant = sqlx::query!(
        "SELECT principal FROM simulation_account_grant WHERE account=$1 AND principal=$2",
        account,
        context.admission.principal
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage)?;
    if grant.is_none() {
        return Err(permanent("execution_unauthorized"));
    }
    // Return digests/booleans, not the potentially 1 MiB plan on every step.
    // PostgreSQL's core SHA-256 verifies retained bytes without an extension.
    let profile = context.profile.identity().1.bytes();
    let config = sqlx::query!(
        r#"SELECT binding,slots,
            sha256(canonical)=binding AS "canonical_ok!",
            sha256(profile)=$2 AS "profile_ok!"
           FROM simulation_execution_config WHERE account=$1"#,
        account,
        &profile[..]
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage)?
    .ok_or_else(|| permanent("execution_configuration"))?;
    if root.configuration != context.admission.configuration.bytes()
        || config.binding != context.binding.bytes()
        || unsigned(config.slots)? != context.steps.len() as u64
        || !config.canonical_ok
        || !config.profile_ok
    {
        return Err(permanent("execution_configuration"));
    }
    let command = serde_json::to_vec(&request).map_err(|_| invariant())?;
    let seq = signed(sequence)?;
    if let Some(receipt) = sqlx::query!(
        "SELECT request FROM simulation_execution_receipt WHERE account=$1 AND sequence=$2",
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
            Err(permanent("execution_identity"))
        };
    }
    let next = unsigned(root.input_revision)?
        .checked_add(1)
        .ok_or_else(invariant)?;
    if sequence > next {
        return Err(HandlerFailure::transient(FailureCode::from_static(
            "execution_input_gap",
        )));
    }
    if sequence != next || expected != unsigned(root.revision)? {
        return Err(permanent("execution_revision"));
    }
    Ok(Some(Fenced {
        root,
        request,
        command,
    }))
}
