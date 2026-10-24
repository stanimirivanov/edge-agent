use super::{invalid, mic};
use crate::{DispatchError, HttpError, HttpErrorKind, JsonDocument, price, volume};
use edgeagent_market_data_application::{DateOnlySchedule, ProviderFailure};
use edgeagent_market_data_domain::{SessionDate, ShareVolume, UsdPrice};
use serde::Deserialize;
use serde_json::value::RawValue;

#[derive(Deserialize)]
struct Status<'a> {
    status: &'a str,
}
#[derive(Deserialize)]
struct Failure {
    code: u16,
}
#[derive(Deserialize)]
struct Response<'a> {
    #[serde(borrow)]
    meta: Meta<'a>,
    #[serde(borrow)]
    values: Vec<Row<'a>>,
}
#[derive(Deserialize)]
struct Meta<'a> {
    symbol: &'a str,
    interval: &'a str,
    currency: &'a str,
    exchange_timezone: &'a str,
    mic_code: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    // Not required by today's protocol. If echoed, it cannot contradict the
    // explicit raw request. Absence is not independent proof of adjustment basis.
    adjust: Option<&'a str>,
    adjusted: Option<bool>,
}
#[derive(Deserialize)]
struct Row<'a> {
    datetime: &'a str,
    #[serde(borrow)]
    open: &'a RawValue,
    #[serde(borrow)]
    high: &'a RawValue,
    #[serde(borrow)]
    low: &'a RawValue,
    #[serde(borrow)]
    close: &'a RawValue,
    #[serde(borrow)]
    volume: &'a RawValue,
}
fn date(value: &str) -> Result<SessionDate, DispatchError> {
    let checked = edgeagent_contracts::evidence::EvidenceDate::new(value).map_err(|_| invalid())?;
    let mut parts = checked.as_str().split('-');
    let mut next = || parts.next().ok_or_else(invalid);
    SessionDate::new(
        next()?.parse().map_err(|_| invalid())?,
        next()?.parse().map_err(|_| invalid())?,
        next()?.parse().map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())
}
type Values = Vec<(SessionDate, [UsdPrice; 4], ShareVolume)>;
pub(super) fn parse(
    document: &JsonDocument,
    schedule: &DateOnlySchedule,
) -> Result<Values, DispatchError> {
    let text = document.root().get();
    let status: Status<'_> = serde_json::from_str(text).map_err(|_| invalid())?;
    if status.status == "error" {
        let failure: Failure = serde_json::from_str(text).map_err(|_| invalid())?;
        let failure = match failure.code {
            401 | 403 => ProviderFailure::Forbidden,
            408 | 429 | 500 | 502 | 503 | 504 => ProviderFailure::Transient {
                retry_after_ms: None,
            },
            _ => ProviderFailure::InvalidResponse,
        };
        return Err(HttpError::new(HttpErrorKind::Provider(failure)).into());
    }
    if status.status != "ok" {
        return Err(invalid());
    }
    let response: Response<'_> = serde_json::from_str(text).map_err(|_| invalid())?;
    let m = response.meta;
    let (symbol, venue, _) = schedule.mapping().value().binding();
    if m.symbol != symbol.as_str()
        || m.mic_code != mic(venue)?
        || m.interval != "1day"
        || m.currency != "USD"
        || m.exchange_timezone != "America/New_York"
        || m.kind != "Common Stock"
        || m.adjust.is_some_and(|mode| mode != "none")
        || m.adjusted == Some(true)
        || response.values.len() != schedule.dates().len()
    {
        return Err(invalid());
    }
    response
        .values
        .into_iter()
        .zip(schedule.dates())
        .map(|(row, expected)| {
            let label = date(row.datetime)?;
            if label != expected {
                return Err(invalid());
            }
            Ok((
                label,
                [
                    price(row.open)?,
                    price(row.high)?,
                    price(row.low)?,
                    price(row.close)?,
                ],
                volume(row.volume)?,
            ))
        })
        .collect()
}
