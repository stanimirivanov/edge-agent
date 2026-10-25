//! Owner-installed transport records. No domain type derives transport serialization.
use super::*;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroU64},
};

pub(super) enum Schedule {
    Native(DateOnlySchedule),
    Precise(PreciseDailySchedule),
}
pub(super) struct Input {
    pub rights: ReferenceHistory<RightsRecord>,
    pub providers: ReferenceHistory<ProviderReview>,
    pub mappings: ReferenceHistory<InstrumentMapping>,
    pub calendars: ReferenceHistory<CalendarDay>,
    pub actions: ReferenceHistory<CorporateAction>,
    pub coverage: ReferenceVersion<ActionCoverage>,
    pub quality: DailyQualityPolicy,
    pub transformation: TransformationRevision,
    pub versions: Vec<ObservationVersion>,
    pub schedule: Schedule,
}
fn invalid<T>(_: T) -> Error {
    Error::new(K::Invalid)
}
fn tuple<const N: usize>(v: Value) -> Result<[Value; N]> {
    let Value::Array(a) = v else {
        return Err(invalid(()));
    };
    a.try_into().map_err(invalid)
}
fn text(v: Value) -> Result<String> {
    serde_json::from_value(v).map_err(invalid)
}
fn u(v: Value) -> Result<u64> {
    v.as_u64().ok_or_else(|| invalid(()))
}
fn i(v: Value) -> Result<i64> {
    v.as_i64().ok_or_else(|| invalid(()))
}
fn id(v: Value) -> Result<u128> {
    let s = text(v)?;
    EvidenceId::new(&s).map_err(invalid)?;
    u128::from_str_radix(&s, 16).map_err(invalid)
}
fn at(v: Value) -> Result<UtcInstant> {
    UtcInstant::from_unix_millis(i(v)?).map_err(invalid)
}
fn date(v: Value) -> Result<SessionDate> {
    let s = text(v)?;
    EvidenceDate::new(&s).map_err(invalid)?;
    let parts = s
        .split('-')
        .map(str::parse::<i32>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(invalid)?;
    let [y, m, d] = parts.as_slice() else {
        return Err(invalid(()));
    };
    SessionDate::new(
        *y,
        u32::try_from(*m).map_err(invalid)?,
        u32::try_from(*d).map_err(invalid)?,
    )
    .map_err(invalid)
}
fn interval(v: Value) -> Result<EffectiveInterval> {
    let [a, b] = tuple(v)?;
    EffectiveInterval::new(at(a)?, at(b)?).map_err(invalid)
}
fn range(v: Value) -> Result<DateRange> {
    let [a, b] = tuple(v)?;
    DateRange::new(date(a)?, date(b)?).map_err(invalid)
}
fn precise(v: Value) -> Result<PreciseInstant> {
    let [s, n] = tuple(v)?;
    PreciseInstant::from_unix_parts(i(s)?, u32::try_from(u(n)?).map_err(invalid)?).map_err(invalid)
}
fn bucket(v: Value) -> Result<EvidenceInterval> {
    let [a, b] = tuple(v)?;
    EvidenceInterval::new(precise(a)?, precise(b)?).map_err(invalid)
}
fn version(v: Value) -> Result<ObservationVersion> {
    let [ident, revision, previous] = tuple(v)?;
    ObservationVersion::new(
        ObservationId::new(id(ident)?).map_err(invalid)?,
        ObservationRevision::new(u(revision)?).map_err(invalid)?,
        if previous.is_null() {
            None
        } else {
            Some(ObservationRevision::new(u(previous)?).map_err(invalid)?)
        },
    )
    .map_err(invalid)
}
fn witness<T>(v: Value, decode: impl FnOnce(Value) -> Result<T>) -> Result<ReferenceVersion<T>> {
    let [ident, revision, source, known, hash, value] = tuple(v)?;
    let s = text(hash)?;
    EvidenceDigest::new(&s).map_err(invalid)?;
    let mut bytes = [0; 32];
    for (byte, pair) in bytes.iter_mut().zip(s.as_bytes().as_chunks::<2>().0) {
        *byte =
            u8::from_str_radix(std::str::from_utf8(pair).map_err(invalid)?, 16).map_err(invalid)?;
    }
    Ok(ReferenceVersion::new(
        ReferenceId::new(id(ident)?).map_err(invalid)?,
        ReferenceRevision::new(u(revision)?).map_err(invalid)?,
        SourceId::new(id(source)?).map_err(invalid)?,
        at(known)?,
        bytes,
        decode(value)?,
    ))
}
fn list<T>(v: Value, max: usize, mut f: impl FnMut(Value) -> Result<T>) -> Result<Vec<T>> {
    let Value::Array(a) = v else {
        return Err(invalid(()));
    };
    if a.len() > max {
        return Err(invalid(()));
    }
    a.into_iter().map(&mut f).collect()
}
fn history<T: Clone + Eq>(
    v: Value,
    f: impl FnMut(Value) -> Result<ReferenceVersion<T>>,
) -> Result<ReferenceHistory<T>> {
    ReferenceHistory::new(list(v, 4096, f)?).map_err(invalid)
}
fn venue(v: Value) -> Result<Venue> {
    match text(v)?.as_str() {
        "XNYS" => Ok(Venue::Xnys),
        "XNAS" => Ok(Venue::Xnas),
        "ARCX" => Ok(Venue::Arcx),
        _ => Err(invalid(())),
    }
}
fn permission(v: Value) -> Result<Permission> {
    match text(v)?.as_str() {
        "allowed" => Ok(Permission::Allowed),
        "denied" => Ok(Permission::Denied),
        "unknown" => Ok(Permission::Unknown),
        _ => Err(invalid(())),
    }
}
fn status(v: Value) -> Result<ReviewStatus> {
    match text(v)?.as_str() {
        "active" => Ok(ReviewStatus::Active),
        "revoked" => Ok(ReviewStatus::Revoked),
        _ => Err(invalid(())),
    }
}
fn mapping(v: Value) -> Result<ReferenceVersion<InstrumentMapping>> {
    witness(v, |v| {
        let [instrument, listing, symbol, mic, dates] = tuple(v)?;
        Ok(InstrumentMapping::new(
            InstrumentId::new(id(instrument)?).map_err(invalid)?,
            ListingId::new(id(listing)?).map_err(invalid)?,
            Symbol::new(&text(symbol)?).map_err(invalid)?,
            venue(mic)?,
            range(dates)?,
        ))
    })
}
fn calendar(v: Value) -> Result<ReferenceVersion<CalendarDay>> {
    witness(v, |v| {
        let [mic, day, kind, window, tz] = tuple(v)?;
        let kind = text(kind)?;
        let hours = match kind.as_str() {
            "closed" if window.is_null() => SessionHours::Closed,
            "regular" | "early_close" => {
                let [open, close] = tuple(window)?;
                let w = SessionWindow::new(at(open)?, at(close)?).map_err(invalid)?;
                if kind == "early_close" {
                    SessionHours::EarlyClose(w)
                } else {
                    SessionHours::Regular(w)
                }
            }
            _ => return Err(invalid(())),
        };
        CalendarDay::new(venue(mic)?, date(day)?, hours, &text(tz)?).map_err(invalid)
    })
}
fn action(v: Value) -> Result<ReferenceVersion<CorporateAction>> {
    witness(v, |v| {
        let [instrument, day, kind, terms] = tuple(v)?;
        let kind = match text(kind)?.as_str() {
            "split" => {
                let [before, after] = tuple(terms)?;
                ActionKind::Split {
                    before: NonZeroU64::new(u(before)?).ok_or_else(|| invalid(()))?,
                    after: NonZeroU64::new(u(after)?).ok_or_else(|| invalid(()))?,
                }
            }
            "cash_dividend" => {
                let [coefficient, scale] = tuple(terms)?;
                ActionKind::CashDividend(
                    UsdPrice::new(
                        text(coefficient)?.parse().map_err(invalid)?,
                        u32::try_from(u(scale)?).map_err(invalid)?,
                    )
                    .map_err(invalid)?,
                )
            }
            "unsupported" if terms.is_null() => ActionKind::Unsupported,
            "cancelled" if terms.is_null() => ActionKind::Cancelled,
            _ => return Err(invalid(())),
        };
        Ok(CorporateAction::new(
            InstrumentId::new(id(instrument)?).map_err(invalid)?,
            date(day)?,
            kind,
        ))
    })
}
pub(super) fn canonical(v: Value) -> Result<Vec<u8>> {
    fn sort(v: Value) -> Value {
        match v {
            Value::Object(m) => Value::Object(
                m.into_iter()
                    .map(|(k, v)| (k, sort(v)))
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.into_iter().map(sort).collect()),
            v => v,
        }
    }
    let mut b = serde_json::to_vec(&sort(v)).map_err(invalid)?;
    b.push(b'\n');
    Ok(b)
}
impl Input {
    pub fn decode(
        bytes: &[u8],
        record: &AdmissionRecord,
        profile: &QualificationProfile,
    ) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > 262144 {
            return Err(invalid(()));
        }
        let value: Value = serde_json::from_slice(bytes).map_err(invalid)?;
        if canonical(value.clone())? != bytes {
            return Err(invalid(()));
        }
        let mut fields: BTreeMap<String, Value> = serde_json::from_value(value).map_err(invalid)?;
        let mut take = |k: &str| fields.remove(k).ok_or_else(|| invalid(()));
        if text(take("version")?)? != "edgeagent.qualification-input.v1" {
            return Err(invalid(()));
        }
        let r = record.request();
        let rights = history(take("rights")?, |v| {
            witness(v, |v| {
                let [
                    agreement,
                    effective,
                    permissions,
                    raw,
                    retention,
                    raw_retention,
                    delete_by,
                    embargo,
                    state,
                ] = tuple(v)?;
                let [automated, derived, storage, audience] = tuple(permissions)?;
                Ok(RightsRecord {
                    scope: r.scope,
                    contract: r.contract,
                    agreement: ReferenceId::new(id(agreement)?).map_err(invalid)?,
                    effective: interval(effective)?,
                    permissions: UsePermissions {
                        automated: permission(automated)?,
                        derived: permission(derived)?,
                        canonical_storage: permission(storage)?,
                        audience: permission(audience)?,
                    },
                    raw_storage: permission(raw)?,
                    canonical_retention_ms: NonZeroU32::new(
                        u32::try_from(u(retention)?).map_err(invalid)?,
                    )
                    .ok_or_else(|| invalid(()))?,
                    raw_retention_ms: if raw_retention.is_null() {
                        None
                    } else {
                        Some(
                            NonZeroU32::new(u32::try_from(u(raw_retention)?).map_err(invalid)?)
                                .ok_or_else(|| invalid(()))?,
                        )
                    },
                    delete_by: at(delete_by)?,
                    embargo_ms: u32::try_from(u(embargo)?).map_err(invalid)?,
                    status: status(state)?,
                })
            })
        })?;
        let providers = history(take("providers")?, |v| {
            witness(v, |v| {
                let [effective, delivery, knowledge, state] = tuple(v)?;
                if text(delivery)? != "snapshots" || text(knowledge)? != "capture_only" {
                    return Err(invalid(()));
                }
                Ok(ProviderReview {
                    scope: r.scope,
                    capability: ProviderCapability::new(
                        r.scope.source,
                        r.contract,
                        DeliveryCapability::Snapshots,
                        KnowledgeCapability::CaptureOnly,
                    ),
                    effective: interval(effective)?,
                    status: status(state)?,
                })
            })
        })?;
        let mappings = history(take("mappings")?, mapping)?;
        let calendars_vec = list(take("calendars")?, 366, calendar)?;
        let calendars = ReferenceHistory::new(calendars_vec.clone()).map_err(invalid)?;
        let actions = history(take("actions")?, action)?;
        let coverage = witness(take("coverage")?, |v| {
            let [instrument, dates, expires] = tuple(v)?;
            Ok(ActionCoverage::new(
                InstrumentId::new(id(instrument)?).map_err(invalid)?,
                range(dates)?,
                at(expires)?,
            ))
        })?;
        let [revision, age, change, difference] = tuple(take("quality")?)?;
        let quality = DailyQualityPolicy::new(
            TransformationRevision::new(u(revision)?).map_err(invalid)?,
            u(age)?,
            u16::try_from(u(change)?).map_err(invalid)?,
            u16::try_from(u(difference)?).map_err(invalid)?,
        )
        .map_err(invalid)?;
        let transformation =
            TransformationRevision::new(u(take("transformation")?)?).map_err(invalid)?;
        let versions = list(take("observations")?, 252, version)?;
        let mut identities = std::collections::BTreeSet::new();
        if versions.iter().any(|v| {
            let (id, revision, predecessor) = v.lineage();
            !identities.insert(id) || revision.get() != 1 || predecessor.is_some()
        }) {
            return Err(invalid(()));
        }
        let [feed, epoch, effective_at, cutoff, session] = tuple(take("context")?)?;
        let scope = RoutingScope {
            source: r.scope.source,
            feed: FeedId::new(id(feed)?).map_err(invalid)?,
            epoch: FeedEpoch::new(u(epoch)?).map_err(invalid)?,
        };
        let context = SelectionContext {
            scope,
            effective_at: at(effective_at)?,
            cutoff: at(cutoff)?,
            session: date(session)?,
        };
        let selections = history(take("selections")?, |v| {
            witness(v, |v| {
                let [effective, members] = tuple(v)?;
                let members = list(members, 32, |v| InstrumentId::new(id(v)?).map_err(invalid))?;
                InstrumentSelection::new(interval(effective)?, &members).map_err(invalid)
            })
        })?;
        let directories = history(take("directories")?, |v| {
            witness(v, |v| {
                let [effective, entries] = tuple(v)?;
                let entries = list(entries, 4096, |v| {
                    let mapping = mapping(v)?;
                    RouteBinding::new(RoutingKey::symbol(mapping.value().binding().0), mapping)
                        .map_err(invalid)
                })?;
                RoutingDirectory::new(scope, interval(effective)?, entries).map_err(invalid)
            })
        })?;
        let [selection, directory] = tuple(take("selection_ids")?)?;
        let plan = SelectionPlan::resolve(
            context,
            (
                ReferenceId::new(id(selection)?).map_err(invalid)?,
                ReferenceId::new(id(directory)?).map_err(invalid)?,
            ),
            &selections,
            &directories,
        )
        .map_err(invalid)?;
        let selected = plan.prepare(r).map_err(invalid)?;
        let buckets = take("buckets")?;
        let schedule = match profile.data().provider {
            QualificationProvider::TwelveDataDaily => {
                let buckets = list(buckets, 252, |v| {
                    witness(v, |v| {
                        let [day, coverage, calendar_id, calendar_revision] = tuple(v)?;
                        DateOnlyBucket::new(
                            date(day)?,
                            bucket(coverage)?,
                            SemanticReference::new(
                                ReferenceId::new(id(calendar_id)?).map_err(invalid)?,
                                ReferenceRevision::new(u(calendar_revision)?).map_err(invalid)?,
                            ),
                        )
                        .map_err(invalid)
                    })
                })?;
                Schedule::Native(
                    DateOnlySchedule::new(&selected, &calendars_vec, &buckets).map_err(invalid)?,
                )
            }
            QualificationProvider::AlpacaHistoricalSipDaily => {
                let slots = list(buckets, 252, |v| {
                    let [day, coverage, observation] = tuple(v)?;
                    Ok(DailyObservationSlot {
                        date: date(day)?,
                        bucket: bucket(coverage)?,
                        version: version(observation)?,
                    })
                })?;
                if slots.iter().map(|s| s.version).collect::<Vec<_>>() != versions {
                    return Err(invalid(()));
                }
                Schedule::Precise(
                    PreciseDailySchedule::new(&selected, &calendars_vec, &slots, transformation)
                        .map_err(invalid)?,
                )
            }
        };
        let digest = match &schedule {
            Schedule::Native(s) => s.configuration_digest(),
            Schedule::Precise(s) => s.configuration_digest(),
        };
        let count = match &schedule {
            Schedule::Native(s) => s.dates().len(),
            Schedule::Precise(s) => s.slots().len(),
        };
        if count != versions.len() {
            return Err(invalid(()));
        }
        if record.configuration() != Some(digest.as_str())
            || digest != &profile.data().configuration_sha256
            || !fields.is_empty()
        {
            return Err(invalid(()));
        }
        let input = Self {
            rights,
            providers,
            mappings,
            calendars,
            actions,
            coverage,
            quality,
            transformation,
            versions,
            schedule,
        };
        let auth = input
            .catalogs()
            .admit(r, r.admitted_at, &Cancellation::default())
            .map_err(invalid)?;
        record.verify(&auth, record.listing()?, quality)?;
        Ok(input)
    }
    pub fn catalogs(&self) -> AuthorityCatalogs<'_> {
        AuthorityCatalogs {
            rights: &self.rights,
            providers: &self.providers,
        }
    }
    pub fn references(&self) -> DailyReferences<'_> {
        DailyReferences {
            mappings: &self.mappings,
            calendar: &self.calendars,
            actions: &self.actions,
            action_coverage: &self.coverage,
        }
    }
}
