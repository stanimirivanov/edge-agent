//! Derive neutral ownership claims from the actual complete provider profile.
use super::*;
use edgeagent_market_data_application::{
    SelectionPlan, StreamNamespaceClaim, StreamOwnerInputs, StreamingSession,
};
impl IexCanonicalProfile {
    /// Compile the exact owner-installable record and derive every namespace claim.
    /// No caller-supplied parallel namespace map can contradict this profile.
    /// The independent owner must install it; this method performs no approval or I/O.
    /// # Errors
    /// Rejects a foreign session pin, invalid expiry or unsupported bounded encoding.
    pub fn owner_inputs(
        &self,
        session: &StreamingSession,
        selection: &SelectionPlan<'_>,
        expires: UtcInstant,
    ) -> Result<StreamOwnerInputs> {
        let routes = selection
            .streaming_routes(session)
            .map_err(|_| IexAcceptanceError::Binding)?;
        let claims = self.namespace_claims()?;
        if session.data().contract != self.contract
            || session.data().delivery != DeliveryCapability::Snapshots
            || session
                .data()
                .scopes
                .iter()
                .any(|s| s.source != self.source)
            || routes
                .iter()
                .any(|r| !self.lanes.iter().any(|l| same(r.mapping(), &l.mapping)))
            || self
                .lanes
                .iter()
                .any(|l| !routes.iter().any(|r| same(r.mapping(), &l.mapping)))
        {
            return Err(IexAcceptanceError::Binding);
        }
        StreamOwnerInputs::new(session, self.bytes(), &claims, expires)
            .map_err(|_| IexAcceptanceError::Binding)
    }
    // Share one complete logical-scope encoding between installation and native projection.
    pub(super) fn namespace_claims(&self) -> Result<Vec<StreamNamespaceClaim>> {
        let mut claims = self
            .lanes
            .iter()
            .map(|lane| {
                let kind = match &lane.scope {
                    IexIdentityScope::Trade { exchange, tape } => {
                        serde_json::json!(["iex-trade", exchange, tape])
                    }
                    IexIdentityScope::MinuteBar => serde_json::json!(["iex-minute-bar"]),
                };
                let bytes = serde_json::to_vec(&serde_json::json!([
                    "edgeagent.iex-identity-scope.v1",
                    format!("{:032x}", self.source.get()),
                    format!("{:032x}", lane.mapping.value().identities().0.get()),
                    lane.calendar.value().key().1.ymd(),
                    kind
                ]))
                .map_err(|_| IexAcceptanceError::Configuration)?;
                StreamNamespaceClaim::new(lane.namespace, &bytes)
                    .map_err(|_| IexAcceptanceError::Configuration)
            })
            .collect::<Result<Vec<_>>>()?;
        claims.sort_by_key(StreamNamespaceClaim::namespace);
        Ok(claims)
    }
}
