//! Array-only canonical encoding is independent of serde_json object-map features.
use super::*;
use serde_json::{Value, json};
fn reference<T>(r: &ReferenceVersion<T>, value: Value) -> Value {
    json!([
        format!("{:032x}", r.id().get()),
        r.revision().get().to_string(),
        format!("{:032x}", r.source().get()),
        r.known_at().unix_millis(),
        r.checksum()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        value
    ])
}
fn venue(v: Venue) -> Result<&'static str> {
    match v {
        Venue::Xnys => Ok("xnys"),
        Venue::Xnas => Ok("xnas"),
        Venue::Arcx => Ok("arcx"),
        Venue::Xiex => Ok("xiex"),
        _ => Err(IexAcceptanceError::Configuration),
    }
}
pub(super) fn encode(
    source: SourceId,
    contract: EvidenceContract,
    transformation: TransformationRevision,
    lanes: &[IexIdentityLane],
    rules: &[IexConditionRule],
) -> Result<Vec<u8>> {
    let mut values = Vec::with_capacity(lanes.len());
    for lane in lanes {
        let m = lane.mapping.value();
        let (symbol, v, dates) = m.binding();
        let (first, last) = dates.bounds();
        let mapping = reference(
            &lane.mapping,
            json!([
                format!("{:032x}", m.identities().0.get()),
                format!("{:032x}", m.identities().1.get()),
                symbol.as_str(),
                venue(v)?,
                first.ymd(),
                last.ymd()
            ]),
        );
        let c = lane.calendar.value();
        let hours = match c.hours() {
            SessionHours::Regular(w) => json!([
                "regular",
                w.bounds().0.unix_millis(),
                w.bounds().1.unix_millis()
            ]),
            SessionHours::EarlyClose(w) => json!([
                "early-close",
                w.bounds().0.unix_millis(),
                w.bounds().1.unix_millis()
            ]),
            _ => return Err(IexAcceptanceError::Configuration),
        };
        let calendar = reference(
            &lane.calendar,
            json!([venue(c.key().0)?, c.key().1.ymd(), hours, c.timezone()]),
        );
        let scope = match &lane.scope {
            IexIdentityScope::Trade { exchange, tape } => json!(["trade", exchange, tape]),
            IexIdentityScope::MinuteBar => json!(["minute-bar"]),
        };
        values.push(json!([
            mapping,
            calendar,
            lane.window.bounds().0.unix_parts(),
            lane.window.bounds().1.unix_parts(),
            lane.namespace.get().to_string(),
            scope
        ]));
    }
    let kind = match contract.shape().0 {
        EvidenceKind::ShareTrade => "trade",
        EvidenceKind::RawBar(BarCoverage::Intraday) => "raw-minute-bar",
        _ => return Err(IexAcceptanceError::Configuration),
    };
    let rules: Vec<_> = rules.iter().map(|r| json!([r.raw, r.canonical])).collect();
    serde_json::to_vec(&json!([
        "edgeagent.iex-canonical-profile.v1",
        format!("{:032x}", source.get()),
        [kind, "xiex", "nanoseconds"],
        format!("{:032x}", contract.rules().0.version().0.get()),
        contract.rules().0.version().1.get().to_string(),
        transformation.get().to_string(),
        values,
        rules
    ]))
    .map_err(|_| IexAcceptanceError::Configuration)
}
