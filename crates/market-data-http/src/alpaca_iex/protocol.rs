//! Strict provider boundary, separate from socket/accounting ownership.
use super::*;
use crate::{JsonDocument, price, volume};
use edgeagent_market_data_application::{EarlyRouter, FrameKind, FrameMetadata, RouteDecision};
use serde::Deserialize;
use serde::de::{DeserializeSeed, SeqAccess, Visitor};
use serde_json::value::RawValue;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum State {
    Welcome,
    Authenticate,
    Subscribe,
    Streaming,
    Stopped,
}
pub(super) struct Decoder<'a> {
    pub state: State,
    router: EarlyRouter<'a>,
    scope: edgeagent_market_data_application::RoutingScope,
    symbols: Vec<String>,
    contract: EvidenceContract,
    batch: u32,
    bars: bool,
}
fn bad() -> IexError {
    IexErrorKind::Protocol.into()
}
struct Rows(usize);
impl<'de> DeserializeSeed<'de> for Rows {
    type Value = Vec<&'de RawValue>;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        d: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for Rows {
    type Value = Vec<&'de RawValue>;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("bounded provider array")
    }
    fn visit_seq<S: SeqAccess<'de>>(
        self,
        mut sequence: S,
    ) -> std::result::Result<Self::Value, S::Error> {
        let mut rows = Vec::new();
        while let Some(row) = sequence.next_element::<&RawValue>()? {
            if rows.len() == self.0 {
                return Err(serde::de::Error::custom("batch capacity"));
            }
            rows.push(row);
        }
        Ok(rows)
    }
}
impl<'a> Decoder<'a> {
    pub fn new(
        session: &StreamingSession,
        selection: &'a SelectionPlan<'a>,
        batch: u32,
    ) -> Result<Self> {
        let data = session.data();
        let (kind, scope) = data.contract.shape();
        if scope != MarketScope::Venue(Venue::Xiex)
            || data.delivery != DeliveryCapability::Snapshots
            || data.contract.rules().1 != TimestampPrecision::Nanoseconds
            || !matches!(
                kind,
                EvidenceKind::ShareTrade | EvidenceKind::RawBar(BarCoverage::Intraday)
            )
            || !(1..=1024).contains(&batch)
        {
            return Err(IexErrorKind::Configuration.into());
        }
        let routes = selection
            .streaming_routes(session)
            .map_err(|_| IexErrorKind::Configuration)?;
        let mut symbols: Vec<_> = routes
            .iter()
            .map(|route| route.mapping().value().binding().0.as_str().to_owned())
            .collect();
        symbols.sort();
        if symbols.is_empty() || symbols.windows(2).any(|s| s.first() == s.get(1)) {
            return Err(IexErrorKind::Configuration.into());
        }
        Ok(Self {
            state: State::Welcome,
            router: EarlyRouter::new(selection, ContinuityMode::Snapshots)
                .map_err(|_| IexErrorKind::Configuration)?,
            scope: selection.routing_scope(),
            symbols,
            contract: data.contract,
            batch,
            bars: matches!(kind, EvidenceKind::RawBar(_)),
        })
    }
    pub fn subscription(&self) -> Result<Vec<u8>> {
        let channel = if self.bars { "bars" } else { "trades" };
        serde_json::to_vec(&serde_json::json!({"action":"subscribe",channel:&self.symbols}))
            .map_err(|_| bad())
    }
    pub fn decode(
        &mut self,
        bytes: &[u8],
        frames: [u64; 2],
        received: UtcInstant,
    ) -> Result<IexProgress> {
        let result = self.checked(bytes, frames, received);
        if result.is_err() {
            self.state = State::Stopped;
            self.router.invalidate()
        }
        result
    }
    fn checked(
        &mut self,
        bytes: &[u8],
        frames: [u64; 2],
        received: UtcInstant,
    ) -> Result<IexProgress> {
        if self.state == State::Stopped {
            return Err(IexErrorKind::Stopped.into());
        }
        let document = JsonDocument::parse(bytes).map_err(|_| bad())?;
        let mut parser = serde_json::Deserializer::from_str(document.root().get());
        let rows = Rows(self.batch as usize)
            .deserialize(&mut parser)
            .map_err(|_| IexErrorKind::Budget)?;
        if rows.is_empty() || rows.len() > self.batch as usize {
            return Err(IexErrorKind::Budget.into());
        }
        let mut records = Vec::new();
        for (item, raw) in rows.iter().enumerate() {
            #[derive(Deserialize)]
            struct Header<'a> {
                #[serde(rename = "T")]
                kind: &'a str,
                #[serde(rename = "S")]
                symbol: Option<&'a str>,
            }
            let h: Header<'_> = serde_json::from_str(raw.get()).map_err(|_| bad())?;
            match h.kind {
                "success" => {
                    if rows.len() != 1 {
                        return Err(bad());
                    }
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Success<'a> {
                        #[serde(rename = "T")]
                        _kind: &'a str,
                        msg: &'a str,
                    }
                    let s: Success<'_> = serde_json::from_str(raw.get()).map_err(|_| bad())?;
                    self.state = match (self.state, s.msg) {
                        (State::Welcome, "connected") => State::Authenticate,
                        (State::Authenticate, "authenticated") => State::Subscribe,
                        _ => return Err(bad()),
                    };
                    return Ok(IexProgress::Control);
                }
                "error" => return Err(IexErrorKind::Provider.into()),
                "subscription" => {
                    if rows.len() != 1 || self.state != State::Subscribe {
                        return Err(bad());
                    }
                    let channels: BTreeMap<String, &RawValue> =
                        serde_json::from_str(raw.get()).map_err(|_| bad())?;
                    for required in ["trades", "bars"] {
                        if !channels.contains_key(required) {
                            return Err(bad());
                        }
                    }
                    if !self.bars
                        && (!channels.contains_key("corrections")
                            || !channels.contains_key("cancelErrors"))
                    {
                        return Err(bad());
                    }
                    for (channel, value) in channels {
                        if channel == "T" {
                            continue;
                        }
                        if ![
                            "trades",
                            "bars",
                            "updatedBars",
                            "quotes",
                            "dailyBars",
                            "statuses",
                            "lulds",
                            "corrections",
                            "cancelErrors",
                        ]
                        .contains(&channel.as_str())
                        {
                            return Err(bad());
                        }
                        let symbols: Vec<&str> =
                            serde_json::from_str(value.get()).map_err(|_| bad())?;
                        let set: BTreeSet<_> = symbols.iter().copied().collect();
                        let expected = channel == if self.bars { "bars" } else { "trades" }
                            || (!self.bars
                                && ["corrections", "cancelErrors"].contains(&channel.as_str()));
                        if set.len() != symbols.len()
                            || if expected {
                                set != self.symbols.iter().map(String::as_str).collect()
                            } else {
                                !set.is_empty()
                            }
                        {
                            return Err(bad());
                        }
                    }
                    self.state = State::Streaming;
                    return Ok(IexProgress::Subscribed);
                }
                "c" | "x" | "u" => return Err(IexErrorKind::Reconciliation.into()),
                "t" | "b" => {}
                _ => return Err(bad()),
            }
            if self.state != State::Streaming || (h.kind == "b") != self.bars {
                return Err(bad());
            }
            let symbol = Symbol::new(h.symbol.ok_or_else(bad)?).map_err(|_| bad())?;
            let route = self
                .router
                .route(&FrameMetadata {
                    scope: self.scope,
                    sequence: None,
                    kind: FrameKind::Data(edgeagent_market_data_application::RoutingKey::symbol(
                        &symbol,
                    )),
                })
                .map_err(|_| bad())?;
            let RouteDecision::Decode(route) = route else {
                continue;
            };
            let (event, value) = parse(raw, self.bars)?;
            if event > PreciseInstant::from_millis(received) {
                return Err(bad());
            }
            if let IexValue::MinuteBar { value, .. } = &value
                && value.bucket().0.bounds().1 > PreciseInstant::from_millis(received)
            {
                return Err(bad());
            }
            records.push(IexObservation {
                mapping: route.mapping().clone(),
                contract: self.contract,
                event_at: event,
                received_at: received,
                frames,
                item: u32::try_from(item).map_err(|_| bad())?,
                value,
            });
        }
        Ok(IexProgress::Records(records))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Trade<'a> {
    #[serde(rename = "T")]
    _kind: &'a str,
    #[serde(rename = "S")]
    _symbol: &'a str,
    i: u64,
    x: &'a str,
    #[serde(borrow)]
    p: &'a RawValue,
    #[serde(borrow)]
    s: &'a RawValue,
    c: Vec<&'a str>,
    t: &'a str,
    z: &'a str,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bar<'a> {
    #[serde(rename = "T")]
    _kind: &'a str,
    #[serde(rename = "S")]
    _symbol: &'a str,
    t: &'a str,
    #[serde(borrow)]
    o: &'a RawValue,
    #[serde(borrow)]
    h: &'a RawValue,
    #[serde(borrow)]
    l: &'a RawValue,
    #[serde(borrow)]
    c: &'a RawValue,
    #[serde(borrow)]
    v: &'a RawValue,
    #[serde(borrow)]
    vw: &'a RawValue,
    n: u64,
}
fn timestamp(s: &str) -> Result<PreciseInstant> {
    if !(20..=30).contains(&s.len()) || !s.ends_with('Z') || s.as_bytes().get(10) != Some(&b'T') {
        return Err(bad());
    }
    let at = chrono::DateTime::parse_from_rfc3339(s).map_err(|_| bad())?;
    PreciseInstant::from_unix_parts(at.timestamp(), at.timestamp_subsec_nanos()).map_err(|_| bad())
}
fn numeric(raw: &RawValue) -> Result<&RawValue> {
    if !raw.get().as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return Err(bad());
    }
    Ok(raw)
}
fn parse(raw: &RawValue, bars: bool) -> Result<(PreciseInstant, IexValue)> {
    if bars {
        let row: Bar<'_> = serde_json::from_str(raw.get()).map_err(|_| bad())?;
        let event = timestamp(row.t)?;
        let (seconds, nanos) = event.unix_parts();
        if seconds.rem_euclid(60) != 0 || nanos != 0 {
            return Err(bad());
        }
        let end = PreciseInstant::from_unix_parts(seconds.checked_add(60).ok_or_else(bad)?, 0)
            .map_err(|_| bad())?;
        let interval = EvidenceInterval::new(event, end).map_err(|_| bad())?;
        let p = |r| price(numeric(r)?).map_err(|_| bad());
        let value = RawBar::new(
            interval,
            BarCoverage::Intraday,
            [p(row.o)?, p(row.h)?, p(row.l)?, p(row.c)?],
            volume(numeric(row.v)?).map_err(|_| bad())?,
        )
        .map_err(|_| bad())?;
        Ok((
            event,
            IexValue::MinuteBar {
                value,
                trades: row.n,
                vwap: p(row.vw)?,
            },
        ))
    } else {
        let row: Trade<'_> = serde_json::from_str(raw.get()).map_err(|_| bad())?;
        if row.x.len() != 1
            || !row.x.bytes().all(|b| b.is_ascii_graphic())
            || !["A", "B", "C"].contains(&row.z)
            || row.c.len() > 16
            || row.c.iter().any(|s| {
                s.is_empty()
                    || s.len() > 4
                    || !s.bytes().all(|b| b.is_ascii() && !b.is_ascii_control())
            })
        {
            return Err(bad());
        }
        let value = ShareTrade::new(
            price(numeric(row.p)?).map_err(|_| bad())?,
            volume(numeric(row.s)?).map_err(|_| bad())?,
        )
        .map_err(|_| bad())?;
        Ok((
            timestamp(row.t)?,
            IexValue::Trade {
                id: row.i,
                value,
                exchange: row.x.to_owned(),
                conditions: row.c.into_iter().map(str::to_owned).collect(),
                tape: row.z.to_owned(),
            },
        ))
    }
}
