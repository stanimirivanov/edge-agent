use super::*;
use crate::{Permission, ReviewStatus};
use edgeagent_market_data_domain::*;
use serde_json::{Value, json};

pub(super) fn seal(
    data: StreamingSessionData,
    selection: &SelectionPlan<'_>,
    catalogs: AuthorityCatalogs<'_>,
    at: UtcInstant,
) -> Result<StreamingSession> {
    let first = data.scopes.first().ok_or_else(invalid)?;
    let duration = data.window.bounds().1.unix_millis() - data.window.bounds().0.unix_millis();
    let limits = data.limits;
    if data.scopes.len() > usize::from(limits.subscriptions.get())
        || limits.subscriptions.get() > 32
        || data
            .scopes
            .iter()
            .zip(data.scopes.iter().skip(1))
            .any(|(a, b)| a.instrument.get() >= b.instrument.get())
        || data.scopes.iter().any(|s| {
            let mut common = *s;
            common.instrument = first.instrument;
            common != *first
        })
        || u64::from(limits.frame_bytes.get()) > limits.bytes.get()
        || limits.bytes.get() > (i64::MAX as u64) - u64::from(u32::MAX)
        || i64::from(limits.connect_ms.get()) > duration
        || i64::from(limits.idle_ms.get()) > duration
        || !data.window.contains(at)
        || !matches!(
            data.contract.shape().0,
            EvidenceKind::RawBar(BarCoverage::CoreSession | BarCoverage::Intraday)
                | EvidenceKind::ShareTrade
                | EvidenceKind::IndexLevel
                | EvidenceKind::OptionSnapshot
        )
        || !matches!(
            data.delivery,
            DeliveryCapability::Snapshots | DeliveryCapability::Sequenced
        )
    {
        return Err(invalid());
    }
    let binding = selection
        .streaming_binding(&data.scopes, data.window, at)
        .map_err(|_| invalid())?;
    let authority = reviews(&data, catalogs, at)?;
    let (kind, market) = data.contract.shape();
    let kind = match kind {
        EvidenceKind::RawBar(BarCoverage::CoreSession) => "core_bar",
        EvidenceKind::RawBar(BarCoverage::Intraday) => "intraday_bar",
        EvidenceKind::ShareTrade => "trade",
        EvidenceKind::IndexLevel => "index",
        EvidenceKind::OptionSnapshot => "option_snapshot",
        _ => return Err(invalid()),
    };
    let market = match market {
        MarketScope::Consolidated => "consolidated",
        MarketScope::IndexPublisher => "index_publisher",
        MarketScope::Venue(Venue::Xnys) => "xnys",
        MarketScope::Venue(Venue::Xnas) => "xnas",
        MarketScope::Venue(Venue::Arcx) => "arcx",
        _ => return Err(invalid()),
    };
    let (semantic, precision) = data.contract.rules();
    let precision = match precision {
        TimestampPrecision::Milliseconds => "ms",
        TimestampPrecision::Microseconds => "us",
        TimestampPrecision::Nanoseconds => "ns",
        _ => return Err(invalid()),
    };
    let scopes: Vec<_> = data
        .scopes
        .iter()
        .map(|s| {
            json!([
                format!("{:032x}", s.principal.get()),
                format!("{:032x}", s.account.get()),
                format!("{:032x}", s.source.get()),
                format!("{:032x}", s.instrument.get()),
                format!("{:032x}", s.endpoint.get()),
                format!("{:032x}", s.audience.get()),
                format!("{:032x}", s.storage.get())
            ])
        })
        .collect();
    let bytes = crate::canonical::canonical(json!({
        "version":"edgeagent.stream-session.v1", "id":data.id.as_str(), "configuration":data.configuration.as_str(),
        "scopes":scopes,"selection":binding,"contract":[kind,market,format!("{:032x}",semantic.version().0.get()),semantic.version().1.get(),precision],
        "delivery": if data.delivery == DeliveryCapability::Sequenced { "sequenced" } else { "snapshots" },
        "window":[data.window.bounds().0.unix_millis(),data.window.bounds().1.unix_millis()],
        "limits":[limits.starts.get(),limits.frames.get(),limits.frame_bytes.get(),limits.bytes.get(),u64::from(limits.subscriptions.get()),u64::from(limits.connect_ms.get()),u64::from(limits.idle_ms.get())],
        "authority":authority
    }))?;
    if bytes.len() > 65536 {
        return Err(invalid());
    }
    let account = first.account;
    Ok(StreamingSession {
        account,
        data,
        authority,
        bytes,
    })
}
fn witness<T>(v: &ReferenceVersion<T>) -> Value {
    json!([
        format!("{:032x}", v.id().get()),
        v.revision().get(),
        format!("{:032x}", v.source().get()),
        v.known_at().unix_millis(),
        v.checksum()
    ])
}
pub(super) fn reviews(
    data: &StreamingSessionData,
    catalogs: AuthorityCatalogs<'_>,
    at: UtcInstant,
) -> Result<Value> {
    let mut selected = Vec::new();
    for scope in &data.scopes {
        let mut rights = catalogs
            .rights
            .at(at)
            .filter(|v| v.value().scope == *scope && v.value().contract == data.contract);
        let r = rights.next().ok_or_else(|| Error::new(K::Denied))?;
        let mut providers = catalogs
            .providers
            .at(at)
            .filter(|v| v.value().scope == *scope);
        let p = providers.next().ok_or_else(|| Error::new(K::Denied))?;
        let grant = r.value();
        let review = p.value();
        let permission = grant.permissions;
        if rights.next().is_some()
            || providers.next().is_some()
            || grant.status != ReviewStatus::Active
            || review.status != ReviewStatus::Active
            || grant.effective.bounds().0 > data.window.bounds().0
            || grant.effective.bounds().1 < data.window.bounds().1
            || review.effective.bounds().0 > data.window.bounds().0
            || review.effective.bounds().1 < data.window.bounds().1
            || grant.delete_by < data.window.bounds().1
            || !grant.effective.contains(at)
            || !review.effective.contains(at)
            || [
                permission.automated,
                permission.derived,
                permission.canonical_storage,
                permission.audience,
            ]
            .iter()
            .any(|v| *v != Permission::Allowed)
            || review.capability.source() != scope.source
            || review
                .capability
                .require(
                    data.contract,
                    data.delivery,
                    KnowledgeCapability::CaptureOnly,
                )
                .is_err()
        {
            return Err(Error::new(K::Denied));
        }
        let delivery = match review.capability.declaration().1 {
            DeliveryCapability::Snapshots => "snapshots",
            DeliveryCapability::Sequenced => "sequenced",
            _ => return Err(invalid()),
        };
        let knowledge = match review.capability.declaration().2 {
            KnowledgeCapability::CaptureOnly => "capture",
            KnowledgeCapability::HistoricalVersions => "historical",
            _ => return Err(invalid()),
        };
        let raw = match grant.raw_storage {
            Permission::Unknown => "unknown",
            Permission::Denied => "denied",
            Permission::Allowed => "allowed",
        };
        selected.push(json!([
            witness(r),
            witness(p),
            format!("{:032x}", grant.agreement.get()),
            grant.effective.bounds().0.unix_millis(),
            grant.effective.bounds().1.unix_millis(),
            grant.delete_by.unix_millis(),
            grant.canonical_retention_ms.get(),
            grant.embargo_ms,
            raw,
            grant.raw_retention_ms.map(std::num::NonZeroU32::get),
            review.effective.bounds().0.unix_millis(),
            review.effective.bounds().1.unix_millis(),
            delivery,
            knowledge
        ]));
    }
    Ok(Value::Array(selected))
}
