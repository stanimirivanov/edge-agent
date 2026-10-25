use super::{invalid, timestamp};
use crate::{DispatchError, JsonDocument, price, volume};
use edgeagent_market_data_application::PreciseDailyRow;
use serde::Deserialize;
use serde_json::value::RawValue;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Page<'a> {
    #[serde(borrow)]
    bars: BTreeMap<&'a str, Vec<Row<'a>>>,
    #[serde(deserialize_with = "required")]
    next_page_token: Option<&'a str>,
}
fn required<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<&'de str>, D::Error> {
    Option::deserialize(d)
}
#[derive(Deserialize)]
struct Row<'a> {
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
}
pub(super) struct ParsedPage {
    pub rows: Vec<PreciseDailyRow>,
    pub next: Option<String>,
}
pub(super) fn parse(document: &JsonDocument, symbol: &str) -> Result<ParsedPage, DispatchError> {
    let page: Page<'_> = serde_json::from_str(document.root().get()).map_err(|_| invalid())?;
    if page.bars.len() != 1 || !page.bars.contains_key(symbol) {
        return Err(invalid());
    }
    let received_at = document.received_at().ok_or_else(invalid)?;
    let rows = page.bars.into_values().next().ok_or_else(invalid)?;
    if rows.len() > 252
        || page.next_page_token.is_some_and(|t| {
            t.is_empty() || t.len() > 1024 || !t.bytes().all(|b| b.is_ascii_graphic())
        })
    {
        return Err(invalid());
    }
    let rows = rows
        .into_iter()
        .map(|row| {
            Ok(PreciseDailyRow {
                event_at: timestamp(row.t)?,
                ohlc: [price(row.o)?, price(row.h)?, price(row.l)?, price(row.c)?],
                volume: volume(row.v)?,
                received_at,
            })
        })
        .collect::<Result<_, DispatchError>>()?;
    Ok(ParsedPage {
        rows,
        next: page.next_page_token.map(str::to_owned),
    })
}
