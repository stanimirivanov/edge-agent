//! Ordering is explicit here: transaction settings, root lock, grant/config,
//! immutable receipt, current versions, authoritative reads, pure checks, writes.
//! All fallible work runs inside the inbox-owned transaction; this module never
//! commits, acknowledges, calls providers or spawns background work.

use super::{AdmissionContext, hex, invariant, permanent, signed, storage, unsigned};
use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    simulation::{AdmissionStatus, DEFINITIONS, OrderAdmission, SubmitDryRunOrder},
};
use edgeagent_inbox_postgres::{PostgresHandlerFuture, PostgresTransactionalMessageHandler};
use edgeagent_market_data_domain::PreciseInstant;
use edgeagent_messaging::{FailureCode, HandlerFailure};
use edgeagent_outbox_postgres::PostgresOutbox;
use edgeagent_simulation_domain::{
    Run,
    orders::{Order, OrderEvent, OrderEventId, OrderEventKind, OrderRejection, OrderSide},
    risk::{
        RiskDecision, RiskIncrease, RiskReason, RiskSnapshot, RiskState, RiskStrategyId, RiskUsage,
        StrategyValuation, UsdCents,
    },
};
use sqlx::{Postgres, Transaction};

impl PostgresTransactionalMessageHandler for AdmissionContext<'_> {
    fn handle<'handler>(
        &'handler self,
        transaction: &'handler mut Transaction<'_, Postgres>,
        envelope: &'handler MessageEnvelope,
    ) -> PostgresHandlerFuture<'handler> {
        Box::pin(async move { admit(self, transaction, envelope).await })
    }
}
async fn admit(
    context: &AdmissionContext<'_>,
    transaction: &mut Transaction<'_, Postgres>,
    envelope: &MessageEnvelope,
) -> Result<(), HandlerFailure> {
    DEFINITIONS[0]
        .validate_envelope(envelope)
        .map_err(|_| permanent("admission_contract"))?;
    let request: SubmitDryRunOrder = envelope
        .payload()
        .map_err(|_| permanent("admission_contract"))?;
    let (account, order_id, quote_id) = request.identity();
    if account != context.account()
        || envelope.partition_key() != Some(format!("account/{account}").as_str())
        || envelope.source() != Component::Gateway.source_uri()
    {
        return Err(permanent("admission_scope"));
    }
    let lock = format!("{}ms", context.limits.lock_ms);
    let statement = format!("{}ms", context.limits.statement_ms);
    let settings=sqlx::query!(r#"SELECT current_setting('transaction_isolation') AS "isolation!", set_config('lock_timeout',$1,true) AS "lock!", set_config('statement_timeout',$2,true) AS "statement!""#,lock,statement)
        .fetch_one(&mut **transaction).await.map_err(storage)?;
    if settings.isolation != "read committed" {
        return Err(permanent("admission_isolation"));
    }
    let root=sqlx::query!("SELECT configuration,revision,input_revision,cash_cents,risk_checkpoint,run_checkpoint FROM simulation_account WHERE account=$1 FOR UPDATE",account)
        .fetch_optional(&mut **transaction).await.map_err(storage)?.ok_or_else(||permanent("admission_account"))?;
    let grant = sqlx::query!(
        "SELECT principal FROM simulation_account_grant WHERE account=$1 AND principal=$2",
        account,
        context.principal
    )
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage)?;
    if grant.is_none() {
        return Err(permanent("admission_unauthorized"));
    }
    if root.configuration != context.configuration.bytes() {
        return Err(permanent("admission_configuration"));
    }
    let command = serde_json::to_vec(&request).map_err(|_| invariant())?;
    if let Some(receipt) = sqlx::query!(
        "SELECT request FROM simulation_order WHERE account=$1 AND order_id=$2",
        account,
        order_id
    )
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage)?
    {
        return if receipt.request == command {
            Ok(())
        } else {
            Err(permanent("admission_identity"))
        };
    }
    let (expected, sequence) = request.position();
    let next = unsigned(root.input_revision)?
        .checked_add(1)
        .ok_or_else(invariant)?;
    if sequence > next {
        return Err(HandlerFailure::transient(FailureCode::from_static(
            "admission_input_gap",
        )));
    }
    if sequence != next || expected != unsigned(root.revision)? {
        return Err(permanent("admission_revision"));
    }
    let quote = context
        .quotes
        .iter()
        .find(|quote| hex(&quote.fingerprint().bytes()) == quote_id)
        .ok_or_else(|| permanent("admission_quote"))?;
    if hex(&quote.order().scope().identity().3.bytes()) != order_id
        || quote.position().0 != sequence
    {
        return Err(permanent("admission_quote_scope"));
    }
    let stored=sqlx::query!("SELECT canonical,input_revision FROM simulation_quote WHERE account=$1 AND fingerprint=$2 AND profile='synthetic-reservation-v1'",account,quote_id)
        .fetch_optional(&mut **transaction).await.map_err(storage)?.ok_or_else(||permanent("admission_quote"))?;
    if stored.canonical != quote.canonical_bytes() || unsigned(stored.input_revision)? != sequence {
        return Err(permanent("admission_quote_identity"));
    }
    crate::audit::ensure(transaction, context, root.revision).await?;
    let at = quote.position().1;
    quote
        .validate_at(at)
        .map_err(|_| permanent("admission_quote_stale"))?;
    let mut run = Run::restore(context.manifest, &root.run_checkpoint).map_err(|_| invariant())?;
    while run.peek().is_some_and(|input| input.available_at() <= at) {
        run.next_input();
    }
    run.advance_to(at)
        .map_err(|_| permanent("admission_input_time"))?;
    let mut risk = RiskState::restore(
        context.account,
        context.policy.clone(),
        context.session,
        &root.risk_checkpoint,
    )
    .map_err(|_| invariant())?;
    let valuations=sqlx::query!("SELECT strategy,gross_cents,loss_cents,valued_seconds,valued_nanos,artifact FROM simulation_valuation WHERE account=$1 ORDER BY strategy LIMIT 33",account)
        .fetch_all(&mut **transaction).await.map_err(storage)?;
    let reservations=sqlx::query!(r#"SELECT strategy,COALESCE(SUM(cash_cents),0)::BIGINT AS "cash!",COUNT(*)::BIGINT AS "orders!" FROM simulation_reservation WHERE account=$1 GROUP BY strategy ORDER BY strategy LIMIT 33"#,account)
        .fetch_all(&mut **transaction).await.map_err(storage)?;
    if valuations.len() > 32
        || reservations.len() > 32
        || reservations.iter().any(|reserved| {
            !valuations
                .iter()
                .any(|row| row.strategy == reserved.strategy)
        })
    {
        return Err(invariant());
    }
    let mut total_reserved = 0_u64;
    let mut rows = Vec::with_capacity(valuations.len());
    for row in &valuations {
        if row.artifact.len() != 32 {
            return Err(invariant());
        }
        let reserved = reservations
            .iter()
            .find(|reserved| reserved.strategy == row.strategy);
        let cash = reserved.map_or(Ok(0), |row| unsigned(row.cash))?;
        let orders = reserved.map_or(Ok(0), |row| unsigned(row.orders))?;
        total_reserved = total_reserved.checked_add(cash).ok_or_else(invariant)?;
        let strategy =
            RiskStrategyId::new(u128::from_str_radix(&row.strategy, 16).map_err(|_| invariant())?)
                .map_err(|_| invariant())?;
        let valued = PreciseInstant::from_unix_parts(
            row.valued_seconds,
            u32::try_from(row.valued_nanos).map_err(|_| invariant())?,
        )
        .map_err(|_| invariant())?;
        let usage = RiskUsage::new(
            UsdCents::new(unsigned(row.gross_cents)?),
            UsdCents::new(cash),
            UsdCents::new(unsigned(row.loss_cents)?),
            orders,
        )
        .map_err(|_| invariant())?;
        rows.push(StrategyValuation::new(strategy, usage, valued));
    }
    let snapshot =
        RiskSnapshot::new(context.account, context.session, &rows).map_err(|_| invariant())?;
    let budget = quote.cash().map_err(|_| invariant())?.get();
    let decision = risk
        .check_increase(
            risk.status().0,
            &snapshot,
            at,
            RiskIncrease::new(quote.order().scope().identity().2, UsdCents::new(budget), 1),
        )
        .map_err(|_| invariant())?;
    let available = unsigned(root.cash_cents)?
        .checked_sub(total_reserved)
        .ok_or_else(invariant)?;
    let strategy = format!("{:032x}", quote.order().scope().identity().2.get());
    let instrument = format!("{:032x}", quote.order().instrument().id().get());
    let unit = super::provision::unit(quote.order().terms().2.parts().0)?;
    let mut quantity = 0_u64;
    let mut status = match decision {
        RiskDecision::Permitted => AdmissionStatus::Admitted,
        RiskDecision::Denied(_) => AdmissionStatus::RiskDenied,
        _ => return Err(invariant()),
    };
    if status == AdmissionStatus::Admitted && budget > available {
        status = AdmissionStatus::InsufficientCash;
    }
    if status == AdmissionStatus::Admitted && quote.order().terms().0 == OrderSide::SellToClose {
        let held=sqlx::query!("SELECT quantity FROM simulation_inventory WHERE account=$1 AND strategy=$2 AND instrument=$3 AND unit=$4",account,strategy,instrument,unit)
            .fetch_optional(&mut **transaction).await.map_err(storage)?;
        let reserved=sqlx::query!(r#"SELECT COALESCE(SUM(quantity),0)::BIGINT AS "quantity!" FROM simulation_reservation WHERE account=$1 AND strategy=$2 AND instrument=$3 AND unit=$4"#,account,strategy,instrument,unit)
            .fetch_one(&mut **transaction).await.map_err(storage)?;
        let held = held.map_or(Ok(0), |row| unsigned(row.quantity))?;
        let remaining = held
            .checked_sub(unsigned(reserved.quantity)?)
            .ok_or_else(invariant)?;
        quantity = quote.order().terms().2.parts().1;
        if quantity > remaining {
            status = AdmissionStatus::InsufficientInventory;
            quantity = 0;
        }
    }
    let (cash, quantity) = if status == AdmissionStatus::Admitted {
        (budget, quantity)
    } else {
        (0, 0)
    };
    let reason = match decision {
        RiskDecision::Denied(reason) => Some(reason_code(reason)?),
        _ => None,
    };
    let outcome = OrderAdmission::new(
        &request,
        &hex(&context.manifest.digest().bytes()),
        &strategy,
        expected + 1,
        status,
        (cash, quantity),
        (risk.status().0, reason),
    )
    .map_err(|_| invariant())?;
    let event_id = run.next_id().map_err(|_| invariant())?;
    let mut order = Order::new(quote.order().clone(), 1).map_err(|_| invariant())?;
    let kind = match status {
        AdmissionStatus::Admitted => OrderEventKind::Accepted,
        AdmissionStatus::RiskDenied => OrderEventKind::Rejected(OrderRejection::Risk),
        _ => OrderEventKind::Rejected(OrderRejection::Funding),
    };
    order
        .apply(
            OrderEvent::new(
                OrderEventId::new(event_id),
                quote.order().scope().identity().3,
                1,
                at,
                kind,
            )
            .map_err(|_| invariant())?,
        )
        .map_err(|_| invariant())?;
    let (seconds, nanos) = at.unix_parts();
    let time =
        chrono::DateTime::from_timestamp(seconds, u32::try_from(nanos).map_err(|_| invariant())?)
            .ok_or_else(invariant)?
            .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    let fact = MessageEnvelope::from_payload(
        MessageMetadata {
            id: super::context::event_identity(
                context.account,
                context.manifest.digest(),
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
            idempotency_key: order_id.into(),
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
    let revision = signed(expected + 1)?;
    let admission_journal = order.journal_bytes(quote.fingerprint());
    sqlx::query!("INSERT INTO simulation_order (account,order_id,quote,request,outcome,account_revision,order_revision,admission_journal) VALUES ($1,$2,$3,$4,$5,$6,1,$7)",account,order_id,quote_id,command,bytes,revision,admission_journal)
        .execute(&mut **transaction).await.map_err(storage)?;
    if status == AdmissionStatus::Admitted {
        let cash = signed(cash)?;
        let quantity = signed(quantity)?;
        sqlx::query!("INSERT INTO simulation_reservation (account,order_id,strategy,instrument,unit,cash_cents,quantity,state) VALUES ($1,$2,$3,$4,$5,$6,$7,'held')",account,order_id,strategy,instrument,unit,cash,quantity)
            .execute(&mut **transaction).await.map_err(storage)?;
    }
    let risk_checkpoint = risk.checkpoint();
    let run_checkpoint = run.checkpoint();
    let sequence = signed(sequence)?;
    let updated=sqlx::query!("UPDATE simulation_account SET revision=$2,input_revision=$3,risk_checkpoint=$4,run_checkpoint=$5 WHERE account=$1 AND revision=$6 AND input_revision=$7",account,revision,sequence,risk_checkpoint,run_checkpoint,root.revision,root.input_revision)
        .execute(&mut **transaction).await.map_err(storage)?;
    if updated.rows_affected() != 1 {
        return Err(invariant());
    }
    crate::audit::capture(transaction, context).await?;
    PostgresOutbox
        .enqueue(transaction, DEFINITIONS[1], &fact)
        .await
        .map_err(|error| {
            HandlerFailure::with_source(
                if error.kind() == edgeagent_outbox_postgres::OutboxErrorKind::Storage {
                    edgeagent_messaging::HandlerFailureKind::Transient
                } else {
                    edgeagent_messaging::HandlerFailureKind::Permanent
                },
                FailureCode::from_static("admission_outbox"),
                error,
            )
        })?;
    Ok(())
}
pub(crate) fn reason_code(reason: RiskReason) -> Result<&'static str, HandlerFailure> {
    Ok(match reason {
        RiskReason::SessionClosed => "session_closed",
        RiskReason::StaleValuation => "stale_valuation",
        RiskReason::AccountGross => "account_gross",
        RiskReason::AccountLoss => "account_loss",
        RiskReason::AccountOrders => "account_orders",
        RiskReason::StrategyGross(_) => "strategy_gross",
        RiskReason::StrategyLoss(_) => "strategy_loss",
        RiskReason::StrategyOrders(_) => "strategy_orders",
        _ => return Err(invariant()),
    })
}
