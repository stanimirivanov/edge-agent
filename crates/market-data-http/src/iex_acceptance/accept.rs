use super::*;
use crate::IexValue;
use edgeagent_market_data_application::{
    AuthorityCatalogs, Cancellation, SelectionPlan, StreamingSession,
};

impl IexCanonicalProfile {
    /// Accept one normalized value after exact profile/selection/current-reference checks.
    /// Unknown conditions, changed references and unsupported updates fail closed. Logical
    /// identity excludes arrival coordinates/receipt times, so replay cannot create a new fact.
    /// # Errors
    /// Returns stable binding/reference/authority/semantic/value failures. Catalog authenticity,
    /// independent admission verification and durable incident handling belong to PR20C2.
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit existing authority, selection and reference boundaries."
    )]
    pub fn accept(
        &self,
        session: &StreamingSession,
        selection: &SelectionPlan<'_>,
        catalogs: AuthorityCatalogs<'_>,
        references: IexAcceptanceReferences<'_>,
        at: UtcInstant,
        cancellation: &Cancellation,
        input: IexObservation,
    ) -> Result<IexAcceptedObservation> {
        session
            .recheck(catalogs, at, cancellation)
            .map_err(|_| IexAcceptanceError::Authority)?;
        if session.data().configuration != self.digest
            || session.data().contract != self.contract
            || session.data().delivery != DeliveryCapability::Snapshots
            || input.contract != self.contract
            || session
                .data()
                .scopes
                .iter()
                .any(|s| s.source != self.source)
        {
            return Err(IexAcceptanceError::Binding);
        }
        let routes = selection
            .streaming_routes(session)
            .map_err(|_| IexAcceptanceError::Binding)?;
        if routes
            .iter()
            .any(|r| !self.lanes.iter().any(|l| same(r.mapping(), &l.mapping)))
            || self
                .lanes
                .iter()
                .any(|l| !routes.iter().any(|r| same(r.mapping(), &l.mapping)))
        {
            return Err(IexAcceptanceError::Binding);
        }
        let scope = match &input.value {
            IexValue::Trade { exchange, tape, .. } => {
                if !code(exchange) || !code(tape) {
                    return Err(IexAcceptanceError::Observation);
                }
                IexIdentityScope::Trade {
                    exchange: exchange.clone(),
                    tape: tape.clone(),
                }
            }
            IexValue::MinuteBar { .. } => IexIdentityScope::MinuteBar,
        };
        let lane = self
            .lanes
            .iter()
            .find(|l| same(&l.mapping, &input.mapping) && l.scope == scope)
            .ok_or(IexAcceptanceError::Semantics)?;
        let (symbol, venue, _) = lane.mapping.value().binding();
        let date = lane.calendar.value().key().1;
        let current = references
            .mappings
            .resolve_mapping(symbol, venue, date, at)
            .map_err(|_| IexAcceptanceError::Reference)?;
        let calendar = references
            .calendars
            .resolve_day(venue, date, at)
            .map_err(|_| IexAcceptanceError::Reference)?;
        if !same(current, &lane.mapping)
            || !same(calendar, &lane.calendar)
            || lane.mapping.known_at() > input.received_at
            || lane.calendar.known_at() > input.received_at
        {
            return Err(IexAcceptanceError::Reference);
        }
        let (start, end) = lane.window.bounds();
        if input.received_at > at
            || !session.data().window.contains(input.received_at)
            || input.event_at < start
            || input.event_at >= end
            || input.frames[0] == 0
            || input.frames[1] < input.frames[0]
            || input.frames[1] > u64::from(session.data().limits.frames.get())
            || input.item >= 1024
        {
            return Err(IexAcceptanceError::Observation);
        }
        let (key, value, conditions) = match &input.value {
            IexValue::Trade {
                id,
                value,
                conditions,
                ..
            } => {
                if *id == 0 || conditions.len() > 16 || conditions.iter().any(|s| !code(s)) {
                    return Err(IexAcceptanceError::Observation);
                }
                let mut codes: Vec<_> = conditions.iter().collect();
                codes.sort();
                if codes.windows(2).any(|w| matches!(w,[a,b] if a==b)) {
                    return Err(IexAcceptanceError::Semantics);
                }
                let rule = self
                    .rules
                    .iter()
                    .find(|r| r.raw.iter().eq(codes.iter().copied()))
                    .ok_or(IexAcceptanceError::Semantics)?;
                (
                    *id,
                    ObservationValue::Trade(*value),
                    rule.canonical.as_slice(),
                )
            }
            IexValue::MinuteBar { value, .. } => {
                let (open, close) = value.bucket().0.bounds();
                let (seconds, nanos) = open.unix_parts();
                if value.bucket().1 != BarCoverage::Intraday
                    || input.event_at != open
                    || nanos != 0
                    || seconds.rem_euclid(60) != 0
                    || close.unix_parts() != (seconds + 60, 0)
                    || close > end
                {
                    return Err(IexAcceptanceError::Observation);
                }
                (
                    u64::try_from(seconds / 60).map_err(|_| IexAcceptanceError::Observation)?,
                    ObservationValue::Bar(*value),
                    &[][..],
                )
            }
        };
        let id = ObservationId::new((u128::from(lane.namespace.get()) << 64) | u128::from(key))
            .map_err(|_| IexAcceptanceError::Observation)?;
        let canonical = CanonicalObservation::new(
            ObservationContext::new(
                lane.mapping.value().identities().0,
                ProviderCapability::new(
                    self.source,
                    self.contract,
                    DeliveryCapability::Snapshots,
                    KnowledgeCapability::CaptureOnly,
                ),
                self.transformation,
                date,
                conditions,
            )
            .map_err(|_| IexAcceptanceError::Observation)?,
            ObservationVersion::new(
                id,
                ObservationRevision::new(1).map_err(|_| IexAcceptanceError::Observation)?,
                None,
            )
            .map_err(|_| IexAcceptanceError::Observation)?,
            EvidenceTimes::new(
                input.event_at,
                None,
                None,
                PreciseInstant::from_millis(input.received_at),
            )
            .map_err(|_| IexAcceptanceError::Observation)?,
            None,
            Some(value),
        )
        .map_err(|_| IexAcceptanceError::Observation)?;
        session
            .recheck(catalogs, at, cancellation)
            .map_err(|_| IexAcceptanceError::Authority)?;
        session
            .recheck(catalogs, at, cancellation)
            .map_err(|_| IexAcceptanceError::Authority)?;
        Ok(IexAcceptedObservation {
            canonical,
            native: input,
            profile: self.digest.clone(),
        })
    }
}
