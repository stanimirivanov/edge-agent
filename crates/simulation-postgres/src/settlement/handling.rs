//! Root fence -> immutable receipt -> canonical postlude -> final evidence ->
//! whole-lot accounting -> order/hold termination -> receipt/seal/checkpoints/outbox.
use super::SettlementContext;
use crate::{hex, invariant, permanent, signed, storage, unsigned};
use edgeagent_contracts::{Component, MessageEnvelope, MessageMetadata, settlement::*};
use edgeagent_inbox_postgres::{PostgresHandlerFuture, PostgresTransactionalMessageHandler};
use edgeagent_messaging::{FailureCode, HandlerFailure, HandlerFailureKind};
use edgeagent_outbox_postgres::PostgresOutbox;
use edgeagent_simulation_domain::{
    Run,
    execution::PositionCost,
    settlement::{DeferredReason, SettlementDecision},
};
use sqlx::{Postgres, Transaction};

impl PostgresTransactionalMessageHandler for SettlementContext<'_, '_, '_, '_> {
    fn handle<'h>(
        &'h self,
        tx: &'h mut Transaction<'_, Postgres>,
        envelope: &'h MessageEnvelope,
    ) -> PostgresHandlerFuture<'h> {
        Box::pin(execute(self, tx, envelope))
    }
}
async fn execute(
    context: &SettlementContext<'_, '_, '_, '_>,
    tx: &mut Transaction<'_, Postgres>,
    envelope: &MessageEnvelope,
) -> Result<(), HandlerFailure> {
    let Some(fenced) = super::fence::acquire(context, tx, envelope).await? else {
        return Ok(());
    };
    let root = fenced.root;
    let request = fenced.request;
    let command = fenced.command;
    let (account, plan, sequence, expected) = request.position();
    let owner = context.execution.admission;
    crate::audit::ensure(tx, owner, root.revision).await?;
    let seq = signed(sequence)?;
    let step = context
        .steps
        .iter()
        .find(|s| s.sequence == sequence)
        .ok_or_else(|| permanent("settlement_step"))?;
    let instrument = format!("{:032x}", step.policy.specification().value().id().get());
    let policy = step.policy.identity().bytes();
    let seal=sqlx::query!("SELECT policy,evidence FROM simulation_settled_instrument WHERE account=$1 AND instrument=$2",account,instrument).fetch_optional(&mut **tx).await.map_err(storage)?;
    let mut run = Run::restore(owner.manifest, &root.run_checkpoint).map_err(|_| invariant())?;
    while run.peek().is_some_and(|i| i.available_at() <= step.at) {
        run.next_input();
    }
    if run.peek().is_some() {
        return Err(permanent("settlement_unconsumed_inputs"));
    }
    run.advance_to(step.at).map_err(|_| invariant())?;
    let mut allocations = Vec::new();
    let mut expired = Vec::new();
    let mut postings = [0_i128; 4];
    let mut evidence = None;
    let mut closing = false;
    let status = if let Some(seal) = seal {
        if seal.policy != policy || seal.evidence.len() != 32 {
            return Err(invariant());
        }
        evidence = Some(hex(&seal.evidence));
        "already_settled"
    } else {
        // Root lock serializes every strategy and every replica. Read the complete
        // instrument position; never round separately per strategy/order.
        let limit = signed(context.limits.allocations as u64 + 1)?;
        let rows=sqlx::query!(r#"SELECT i.strategy,i.unit,i.quantity,c.cost_cents AS "cost_cents?" FROM simulation_inventory i LEFT JOIN simulation_position_cost c USING(account,strategy,instrument,unit) WHERE i.account=$1 AND i.instrument=$2 ORDER BY i.strategy COLLATE "C" LIMIT $3"#,account,instrument,limit).fetch_all(&mut **tx).await.map_err(storage)?;
        if rows.len() > context.limits.allocations {
            return Err(permanent("settlement_capacity"));
        }
        let mut quantity = 0_u64;
        let mut cost = 0_u64;
        for row in &rows {
            if row.unit != "contracts" {
                return Err(invariant());
            }
            let q = unsigned(row.quantity)?;
            let c = unsigned(row.cost_cents.ok_or_else(invariant)?)?;
            PositionCost::new(q, c).map_err(|_| invariant())?;
            quantity = quantity.checked_add(q).ok_or_else(invariant)?;
            cost = cost.checked_add(c).ok_or_else(invariant)?;
        }
        let holds=sqlx::query!(r#"SELECT COALESCE(SUM(cash_cents),0)::BIGINT AS "cash!" FROM simulation_reservation WHERE account=$1 AND NOT(instrument=$2 AND unit='contracts')"#,account,instrument).fetch_one(&mut **tx).await.map_err(storage)?;
        let available = unsigned(root.cash_cents)?
            .checked_sub(unsigned(holds.cash)?)
            .ok_or_else(invariant)?;
        match step
            .policy
            .calculate_account(
                step.at,
                PositionCost::new(quantity, cost).map_err(|_| invariant())?,
                available,
                &step.fixings,
            )
            .map_err(|_| permanent("settlement_accounting"))?
        {
            SettlementDecision::Deferred(reason) => match reason {
                DeferredReason::NotDue => "deferred_not_due",
                DeferredReason::MissingEvidence => "deferred_missing",
                DeferredReason::NotKnown => "deferred_not_known",
                _ => return Err(invariant()),
            },
            SettlementDecision::Calculated(result) => {
                expired =
                    super::orders::expire(context, tx, &instrument, step.at, &mut run).await?;
                let left=sqlx::query!(r#"SELECT COUNT(*) AS "count!" FROM simulation_reservation WHERE account=$1 AND instrument=$2 AND unit='contracts'"#,account,instrument).fetch_one(&mut **tx).await.map_err(storage)?;
                if left.count != 0 {
                    return Err(invariant());
                }
                let positions = rows
                    .iter()
                    .map(|row| {
                        let strategy = edgeagent_simulation_domain::risk::RiskStrategyId::new(
                            u128::from_str_radix(&row.strategy, 16).map_err(|_| invariant())?,
                        )
                        .map_err(|_| invariant())?;
                        Ok((
                            strategy,
                            PositionCost::new(
                                unsigned(row.quantity)?,
                                unsigned(row.cost_cents.ok_or_else(invariant)?)?,
                            )
                            .map_err(|_| invariant())?,
                        ))
                    })
                    .collect::<Result<Vec<_>, HandlerFailure>>()?;
                for allocation in result.allocate(&positions).map_err(|_| invariant())? {
                    let (strategy, units, delta_g, delta_f, effect) = allocation.parts();
                    let strategy = format!("{:032x}", strategy.get());
                    let [cash, cost, realized, fee] = effect.map(i64::try_from);
                    let debits = [
                        cash.map_err(|_| invariant())?,
                        cost.map_err(|_| invariant())?,
                        realized.map_err(|_| invariant())?,
                        fee.map_err(|_| invariant())?,
                    ];
                    allocations.push(
                        SettlementAllocation::new(&strategy, units, delta_g, delta_f, debits)
                            .map_err(|_| invariant())?,
                    );
                    crate::execution::persistence::position(
                        tx,
                        account,
                        (&strategy, &instrument, "contracts"),
                        PositionCost::new(0, 0).map_err(|_| invariant())?,
                        effect,
                    )
                    .await?;
                    for (total, value) in postings.iter_mut().zip(effect) {
                        *total = total.checked_add(value).ok_or_else(invariant)?;
                    }
                }
                if postings != result.effects().1 {
                    return Err(invariant());
                }
                evidence = Some(hex(&result.provenance().1.bytes()));
                closing = true;
                "settled"
            }
            _ => return Err(invariant()),
        }
    };
    let cash = i128::from(root.cash_cents)
        .checked_add(postings[0])
        .and_then(|n| i64::try_from(n).ok())
        .filter(|n| *n >= 0)
        .ok_or_else(invariant)?;
    let risk =
        crate::execution::persistence::risk(tx, context.execution, &root.risk_checkpoint, step.at)
            .await?;
    let fact = AccountSettlement::new(
        request.clone(),
        &instrument,
        status,
        (
            &hex(&owner.manifest.digest().bytes()),
            &hex(&policy),
            evidence.as_deref(),
        ),
        allocations,
        expired,
    )
    .map_err(|_| invariant())?;
    let event = run.next_id().map_err(|_| invariant())?;
    let (s, n) = step.at.unix_parts();
    let time = chrono::DateTime::from_timestamp(s, u32::try_from(n).map_err(|_| invariant())?)
        .ok_or_else(invariant)?
        .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    let envelope_out = MessageEnvelope::from_payload(
        MessageMetadata {
            id: crate::context::event_identity(
                owner.account,
                owner.manifest.digest(),
                event.bytes(),
            ),
            source: Component::ExecutionSimulator.source_uri().into(),
            message_type: DEFINITIONS[1].message_type().into(),
            subject: format!("account/{account}"),
            time,
            data_schema: DEFINITIONS[1].data_schema().into(),
            correlation_id: envelope
                .extension("correlationid")
                .ok_or_else(invariant)?
                .into(),
            causation_id: envelope.id().into(),
            idempotency_key: format!("{plan}:{sequence}"),
            partition_key: format!("account/{account}"),
            trace_parent: envelope
                .extension("traceparent")
                .ok_or_else(invariant)?
                .into(),
            trace_state: envelope.extension("tracestate").map(str::to_owned),
        },
        &fact,
    )
    .map_err(|_| invariant())?;
    let outcome = envelope_out.to_json().map_err(|_| invariant())?;
    if outcome.len() > 16384 {
        return Err(invariant());
    }
    let [cash_delta, position_delta, realized_delta, fee_delta] = postings.map(i64::try_from);
    let cash_delta = cash_delta.map_err(|_| invariant())?;
    let position_delta = position_delta.map_err(|_| invariant())?;
    let realized_delta = realized_delta.map_err(|_| invariant())?;
    let fee_delta = fee_delta.map_err(|_| invariant())?;
    sqlx::query!("INSERT INTO simulation_settlement_receipt(account,sequence,request,outcome,cash_delta,position_delta,realized_delta,fee_delta) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",account,seq,command,outcome,cash_delta,position_delta,realized_delta,fee_delta).execute(&mut **tx).await.map_err(storage)?;
    if closing {
        let fixing = step
            .fixings
            .first()
            .ok_or_else(invariant)?
            .identity()
            .bytes();
        sqlx::query!("INSERT INTO simulation_settled_instrument(account,instrument,policy,evidence,sequence) VALUES($1,$2,$3,$4,$5)",account,instrument,&policy[..],&fixing[..],seq).execute(&mut **tx).await.map_err(storage)?;
    }
    let revision = signed(expected.checked_add(1).ok_or_else(invariant)?)?;
    let checkpoint = run.checkpoint();
    let changed=sqlx::query!("UPDATE simulation_account SET revision=$2,input_revision=$3,cash_cents=$4,risk_checkpoint=$5,run_checkpoint=$6 WHERE account=$1 AND revision=$7 AND input_revision=$8",account,revision,seq,cash,risk,checkpoint,root.revision,root.input_revision).execute(&mut **tx).await.map_err(storage)?;
    if changed.rows_affected() != 1 {
        return Err(invariant());
    }
    crate::audit::capture(tx, owner).await?;
    PostgresOutbox
        .enqueue(tx, DEFINITIONS[1], &envelope_out)
        .await
        .map_err(|e| {
            HandlerFailure::with_source(
                if e.kind() == edgeagent_outbox_postgres::OutboxErrorKind::Storage {
                    HandlerFailureKind::Transient
                } else {
                    HandlerFailureKind::Permanent
                },
                FailureCode::from_static("settlement_outbox"),
                e,
            )
        })?;
    Ok(())
}
