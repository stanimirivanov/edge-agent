//! Inbox-owned transaction: root fence -> identity -> evidence/liquidity fence ->
//! pure lifecycle/accounting -> effects/checkpoints/receipt/outbox. Never commits.
use super::{ExecutionAction, ExecutionContext};
use crate::{hex, invariant, permanent, signed, storage, unsigned};
use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    execution::{DEFINITIONS, ExecutionDecision},
};
use edgeagent_inbox_postgres::{PostgresHandlerFuture, PostgresTransactionalMessageHandler};
use edgeagent_messaging::{FailureCode, HandlerFailure};
use edgeagent_outbox_postgres::PostgresOutbox;
use edgeagent_simulation_domain::{
    ArtifactDigest, Run,
    execution::{ExecutableLiquidity, FillAccounting, LiquidityExecution, PositionCost},
    orders::*,
};
use sqlx::{Postgres, Transaction};

impl PostgresTransactionalMessageHandler for ExecutionContext<'_, '_, '_> {
    fn handle<'h>(
        &'h self,
        tx: &'h mut Transaction<'_, Postgres>,
        envelope: &'h MessageEnvelope,
    ) -> PostgresHandlerFuture<'h> {
        Box::pin(async move { execute(self, tx, envelope).await })
    }
}
async fn execute(
    context: &ExecutionContext<'_, '_, '_>,
    tx: &mut Transaction<'_, Postgres>,
    envelope: &MessageEnvelope,
) -> Result<(), HandlerFailure> {
    let Some(super::fence::Fenced {
        root,
        request,
        command,
    }) = super::fence::acquire(context, tx, envelope).await?
    else {
        return Ok(());
    };
    let (account, plan, sequence, expected) = request.position();
    crate::audit::ensure(tx, context.admission, root.revision).await?;
    let seq = signed(sequence)?;
    let step = context
        .steps
        .iter()
        .find(|s| s.sequence == sequence)
        .ok_or_else(|| permanent("execution_step"))?;
    let quote = context
        .admission
        .quotes
        .iter()
        .find(|q| q.fingerprint() == step.quote)
        .ok_or_else(invariant)?;
    let definition = quote.order();
    let order_id = hex(&definition.scope().identity().3.bytes());
    let submission = sqlx::query!(
        "SELECT admission_journal FROM simulation_order WHERE account=$1 AND order_id=$2",
        account,
        order_id
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage)?
    .ok_or_else(|| permanent("execution_not_admitted"))?;
    let stored=sqlx::query!("SELECT journal,exact_cents,filled_units,gross_cents,fees_cents FROM simulation_execution_order WHERE account=$1 AND order_id=$2",account,order_id).fetch_optional(&mut **tx).await.map_err(storage)?;
    let initial = submission
        .admission_journal
        .ok_or_else(|| permanent("execution_legacy_order"))?;
    let journal = stored
        .as_ref()
        .map_or(initial.as_slice(), |s| s.journal.as_slice());
    let mut order = Order::restore_journal(definition.clone(), 4096, quote.fingerprint(), journal)
        .map_err(|_| invariant())?;
    let previous = stored
        .as_ref()
        .map_or(Ok((0_u128, 0_u64, 0_u64, 0_u64)), |s| {
            Ok((
                s.exact_cents.parse::<u128>().map_err(|_| invariant())?,
                unsigned(s.filled_units)?,
                unsigned(s.gross_cents)?,
                unsigned(s.fees_cents)?,
            ))
        })?;
    if FillAccounting::retained_totals(&order, context.profile).map_err(|_| invariant())?
        != previous
    {
        return Err(invariant());
    }
    let reservation=sqlx::query!("SELECT strategy,instrument,unit,cash_cents,quantity FROM simulation_reservation WHERE account=$1 AND order_id=$2",account,order_id).fetch_optional(&mut **tx).await.map_err(storage)?;
    let already_terminal = matches!(
        order.state().0,
        OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Expired | OrderStatus::Rejected
    );
    if already_terminal == reservation.is_some() {
        return Err(invariant());
    }
    let strategy = format!("{:032x}", definition.scope().identity().2.get());
    let instrument = format!("{:032x}", definition.instrument().id().get());
    let unit = crate::provision::unit(definition.terms().2.parts().0)?;
    if reservation
        .as_ref()
        .is_some_and(|r| r.strategy != strategy || r.instrument != instrument || r.unit != unit)
    {
        return Err(invariant());
    }
    let mut run =
        Run::restore(context.admission.manifest, &root.run_checkpoint).map_err(|_| invariant())?;
    while run.peek().is_some_and(|i| i.available_at() <= step.at) {
        run.next_input();
    }
    run.advance_to(step.at)
        .map_err(|_| permanent("execution_time"))?;
    let mut liquidity = None;
    let mut consumed = None;
    let mut evidence = None;
    let mut budget_key = None;
    let deadline = definition.lifetime().bounds().1;
    let pending = order
        .events()
        .iter()
        .rev()
        .find(|e| e.effect().1 == OrderEventKind::CancelRequested)
        .map(|e| e.effect().0);
    let cancel_due = pending
        .map(|at| context.profile.cancellation_due(at))
        .transpose()
        .map_err(|_| invariant())?;
    let event_id = run.next_id().map_err(|_| invariant())?;
    // A scheduled operation may lose to a prior terminal fact or denied
    // admission. Retain a no-effect decision; never strand the account's next
    // canonical input or manufacture another lifecycle transition.
    let kind = if already_terminal {
        None
    } else {
        Some(if step.at >= deadline {
            OrderEventKind::Expired
        } else if order.state().0 == OrderStatus::CancelPending
            && cancel_due.is_some_and(|due| step.at > due)
        {
            OrderEventKind::Cancelled
        } else {
            match &step.action {
                ExecutionAction::RequestCancel => OrderEventKind::CancelRequested,
                ExecutionAction::ConfirmCancel => {
                    if !cancel_due.is_some_and(|due| step.at >= due) {
                        return Err(permanent("execution_cancel_early"));
                    }
                    OrderEventKind::Cancelled
                }
                ExecutionAction::Expire => return Err(permanent("execution_expiry_early")),
                ExecutionAction::Attempt(ready, artifacts) => {
                    let market = ExecutableLiquidity::from_ready(
                        &run,
                        definition,
                        context.profile,
                        ready,
                        artifacts,
                    )
                    .map_err(|_| permanent("execution_evidence"))?;
                    let (key, content, capacity) = market.budget();
                    let run_id = run.manifest_digest().bytes();
                    let key_bytes = key.bytes();
                    let content_bytes = content.bytes();
                    let cap = signed(capacity)?;
                    // Insert/lock one run-wide budget after the account root. No lock
                    // cycle can acquire another account root while holding this row.
                    sqlx::query!("INSERT INTO simulation_liquidity(run,budget,content,capacity,consumed) VALUES($1,$2,$3,$4,0) ON CONFLICT(run,budget) DO NOTHING",&run_id[..],&key_bytes[..],&content_bytes[..],cap).execute(&mut **tx).await.map_err(storage)?;
                    let row=sqlx::query!("SELECT content,capacity,consumed FROM simulation_liquidity WHERE run=$1 AND budget=$2 FOR UPDATE",&run_id[..],&key_bytes[..]).fetch_one(&mut **tx).await.map_err(storage)?;
                    if row.content != content_bytes || row.capacity != cap {
                        return Err(permanent("execution_liquidity_conflict"));
                    }
                    consumed = Some(unsigned(row.consumed)?);
                    budget_key = Some(key);
                    let mut provenance = b"EA-EXECUTION-EVIDENCE1\0".to_vec();
                    for artifact in artifacts {
                        provenance.extend(artifact.bytes());
                    }
                    let digest = ArtifactDigest::of(&provenance);
                    evidence = Some(digest);
                    let mut model = LiquidityExecution::new(
                        market,
                        unsigned(row.consumed)?,
                        quote.protection().0,
                        quote.position().1,
                    )
                    .map_err(|_| invariant())?;
                    let candidate = order
                        .simulate(
                            &run,
                            &mut model,
                            SimulationStep::new(
                                OrderEventId::new(event_id),
                                order.state().1 + 1,
                                ready,
                                context.profile.identity(),
                                digest,
                                artifacts,
                            )
                            .map_err(|_| invariant())?,
                        )
                        .map_err(|_| permanent("execution_model"))?;
                    let kind = candidate.effect().1;
                    liquidity = if let OrderEventKind::Filled(fill) = kind {
                        Some(fill.evidence().0)
                    } else {
                        None
                    };
                    kind
                }
            }
        })
    };
    if let Some(kind) = kind {
        order
            .apply(
                OrderEvent::new(
                    OrderEventId::new(event_id),
                    definition.scope().identity().3,
                    order.state().1 + 1,
                    step.at,
                    kind,
                )
                .map_err(|_| invariant())?,
            )
            .map_err(|_| permanent("execution_lifecycle"))?;
    }
    let mut totals = previous;
    let mut postings = [0_i128; 4];
    let mut filled = 0_u64;
    if let Some(proposal) = liquidity {
        let (quantity, price) = proposal.values();
        filled = quantity.parts().1;
        let position=sqlx::query!("SELECT i.quantity,c.cost_cents FROM simulation_inventory i JOIN simulation_position_cost c USING(account,strategy,instrument,unit) WHERE i.account=$1 AND i.strategy=$2 AND i.instrument=$3 AND i.unit=$4",account,strategy,instrument,unit).fetch_one(&mut **tx).await.map_err(storage)?;
        let position =
            PositionCost::new(unsigned(position.quantity)?, unsigned(position.cost_cents)?)
                .map_err(|_| invariant())?;
        let accounting = FillAccounting::calculate(
            definition,
            context.profile,
            price,
            filled,
            (previous.0, previous.1),
            quote.protection().1.get(),
            position,
        )
        .map_err(|_| permanent("execution_accounting"))?;
        totals = accounting.totals();
        let (position, effects) = accounting.effects();
        postings = effects;
        super::persistence::position(
            tx,
            account,
            (&strategy, &instrument, unit),
            position,
            postings,
        )
        .await?;
        let run_id = run.manifest_digest().bytes();
        let key = budget_key.ok_or_else(invariant)?.bytes();
        let used = signed(
            consumed
                .ok_or_else(invariant)?
                .checked_add(filled)
                .ok_or_else(invariant)?,
        )?;
        sqlx::query!(
            "UPDATE simulation_liquidity SET consumed=$3 WHERE run=$1 AND budget=$2",
            &run_id[..],
            &key[..],
            used
        )
        .execute(&mut **tx)
        .await
        .map_err(storage)?;
    }
    let cash = i128::from(root.cash_cents)
        .checked_add(postings[0])
        .and_then(|n| i64::try_from(n).ok())
        .filter(|n| *n >= 0)
        .ok_or_else(invariant)?;
    let terminal = matches!(
        order.state().0,
        OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Expired | OrderStatus::Rejected
    );
    if let Some(reservation) = &reservation {
        let spent = if definition.terms().0 == OrderSide::BuyToOpen {
            totals
                .2
                .checked_sub(previous.2)
                .and_then(|n| n.checked_add(totals.3.checked_sub(previous.3)?))
                .ok_or_else(invariant)?
        } else {
            totals.3.checked_sub(previous.3).ok_or_else(invariant)?
        };
        let held = signed(
            unsigned(reservation.cash_cents)?
                .checked_sub(spent)
                .ok_or_else(invariant)?,
        )?;
        let inventory = if definition.terms().0 == OrderSide::SellToClose {
            signed(
                unsigned(reservation.quantity)?
                    .checked_sub(filled)
                    .ok_or_else(invariant)?,
            )?
        } else {
            0
        };
        if terminal {
            sqlx::query!(
                "DELETE FROM simulation_reservation WHERE account=$1 AND order_id=$2",
                account,
                order_id
            )
            .execute(&mut **tx)
            .await
            .map_err(storage)?;
        } else {
            sqlx::query!("UPDATE simulation_reservation SET cash_cents=$3,quantity=$4 WHERE account=$1 AND order_id=$2",account,order_id,held,inventory).execute(&mut **tx).await.map_err(storage)?;
        }
    }
    let risk = super::persistence::risk(tx, context, &root.risk_checkpoint, step.at).await?;
    let journal = order.journal_bytes(quote.fingerprint());
    let exact = totals.0.to_string();
    let units = signed(totals.1)?;
    let gross = signed(totals.2)?;
    let fees = signed(totals.3)?;
    sqlx::query!("INSERT INTO simulation_execution_order(account,order_id,journal,exact_cents,filled_units,gross_cents,fees_cents) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(account,order_id) DO UPDATE SET journal=EXCLUDED.journal,exact_cents=EXCLUDED.exact_cents,filled_units=EXCLUDED.filled_units,gross_cents=EXCLUDED.gross_cents,fees_cents=EXCLUDED.fees_cents",account,order_id,journal,exact,units,gross,fees).execute(&mut **tx).await.map_err(storage)?;
    let status = match kind {
        None => "already_terminal",
        Some(OrderEventKind::Filled(_)) if terminal => "filled",
        Some(OrderEventKind::Filled(_)) => "partial_fill",
        Some(OrderEventKind::NoFill { .. }) => "no_fill",
        Some(OrderEventKind::CancelRequested) => "cancel_pending",
        Some(OrderEventKind::Cancelled) => "cancelled",
        Some(OrderEventKind::Expired) => "expired",
        _ => return Err(invariant()),
    };
    let evidence = evidence.map(|e| hex(&e.bytes()));
    let model = hex(&context.profile.identity().1.bytes());
    let debits = postings.map(i64::try_from);
    let [
        cash_posting,
        position_posting,
        realized_posting,
        fee_posting,
    ] = debits;
    let debits = [
        cash_posting.map_err(|_| invariant())?,
        position_posting.map_err(|_| invariant())?,
        realized_posting.map_err(|_| invariant())?,
        fee_posting.map_err(|_| invariant())?,
    ];
    let outcome = ExecutionDecision::new(
        request.clone(),
        &order_id,
        order.state().1,
        status,
        (filled, debits),
        (&model, evidence.as_deref()),
        (
            &strategy,
            &instrument,
            unit,
            if definition.terms().0 == OrderSide::BuyToOpen {
                "buy_to_open"
            } else {
                "sell_to_close"
            },
        ),
    )
    .map_err(|_| invariant())?;
    let (s, n) = step.at.unix_parts();
    let time = chrono::DateTime::from_timestamp(s, u32::try_from(n).map_err(|_| invariant())?)
        .ok_or_else(invariant)?
        .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    let fact = MessageEnvelope::from_payload(
        MessageMetadata {
            id: crate::context::event_identity(
                context.admission.account,
                context.admission.manifest.digest(),
                event_id.bytes(),
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
        &outcome,
    )
    .map_err(|_| invariant())?;
    let bytes = fact.to_json().map_err(|_| invariant())?;
    super::persistence::receipt(tx, account, seq, &command, &bytes, postings).await?;
    let revision = signed(expected + 1)?;
    let run_checkpoint = run.checkpoint();
    let changed=sqlx::query!("UPDATE simulation_account SET revision=$2,input_revision=$3,cash_cents=$4,risk_checkpoint=$5,run_checkpoint=$6 WHERE account=$1 AND revision=$7 AND input_revision=$8",account,revision,seq,cash,risk,run_checkpoint,root.revision,root.input_revision).execute(&mut **tx).await.map_err(storage)?;
    if changed.rows_affected() != 1 {
        return Err(invariant());
    }
    crate::audit::capture(tx, context.admission).await?;
    PostgresOutbox
        .enqueue(tx, DEFINITIONS[1], &fact)
        .await
        .map_err(|e| {
            HandlerFailure::with_source(
                if e.kind() == edgeagent_outbox_postgres::OutboxErrorKind::Storage {
                    edgeagent_messaging::HandlerFailureKind::Transient
                } else {
                    edgeagent_messaging::HandlerFailureKind::Permanent
                },
                FailureCode::from_static("execution_outbox"),
                e,
            )
        })?;
    Ok(())
}
