//! Bounded canonical decoder; no decoding path grants current authority.
use super::*;
use std::num::NonZeroU32;

impl AdmissionRecord {
    /// Recover a bounded canonical record using an authenticated owner-retained command reference.
    /// This verifies identity, not present-day permission; recheck current catalogs before I/O.
    /// # Errors
    /// Rejects unknown/missing fields, malformed values, noncanonical bytes or digest mismatch.
    pub fn decode(bytes: &[u8], expected: &AcquireEvidence) -> Result<Self, Error> {
        if bytes.len() > 8192 || DailyEvidenceArtifact::hash(bytes) != expected.request_sha256 {
            return Err(Error::new(Kind::Invalid));
        }
        let m: BTreeMap<String, String> =
            serde_json::from_slice(bytes).map_err(|_| Error::new(Kind::Invalid))?;
        let get = |key: &str| {
            m.get(key)
                .map(String::as_str)
                .ok_or_else(|| Error::new(Kind::Invalid))
        };
        let id = |key| {
            let text = get(key)?;
            EvidenceId::new(text).map_err(|_| Error::new(Kind::Invalid))?;
            u128::from_str_radix(text, 16).map_err(|_| Error::new(Kind::Invalid))
        };
        let n = |key| {
            get(key)?
                .parse::<u32>()
                .map_err(|_| Error::new(Kind::Invalid))
        };
        let positive = |key| NonZeroU32::new(n(key)?).ok_or_else(|| Error::new(Kind::Invalid));
        let at = |key| {
            UtcInstant::from_unix_millis(get(key)?.parse().map_err(|_| Error::new(Kind::Invalid))?)
                .map_err(|_| Error::new(Kind::Invalid))
        };
        let date = |key| {
            let mut parts = get(key)?.split('-');
            let mut next = || parts.next().ok_or_else(|| Error::new(Kind::Invalid));
            let y = next()?.parse().map_err(|_| Error::new(Kind::Invalid))?;
            let mn = next()?.parse().map_err(|_| Error::new(Kind::Invalid))?;
            let d = next()?.parse().map_err(|_| Error::new(Kind::Invalid))?;
            if parts.next().is_some() {
                return Err(Error::new(Kind::Invalid));
            }
            SessionDate::new(y, mn, d).map_err(|_| Error::new(Kind::Invalid))
        };
        let convert = |_| Error::new(Kind::Invalid);
        if !matches!(
            get("format")?,
            "edgeagent.admission.daily.v1"
                | "edgeagent.admission.daily.v2"
                | "edgeagent.admission.date-only.v1"
        ) {
            return Err(Error::new(Kind::Invalid));
        }
        let request = AcquisitionRequest {
            id: AcquisitionId::new(id("request")?).map_err(convert)?,
            scope: AccessScope {
                principal: PrincipalId::new(id("principal")?).map_err(convert)?,
                account: ProviderAccountId::new(id("account")?).map_err(convert)?,
                source: SourceId::new(id("source")?).map_err(convert)?,
                instrument: InstrumentId::new(id("instrument")?).map_err(convert)?,
                endpoint: ReferenceId::new(id("endpoint")?).map_err(convert)?,
                audience: AudienceId::new(id("audience")?).map_err(convert)?,
                storage: StorageScopeId::new(id("storage")?).map_err(convert)?,
            },
            range: DateRange::new(date("start")?, date("end")?)
                .map_err(|_| Error::new(Kind::Invalid))?,
            contract: EvidenceContract::new(
                EvidenceKind::RawBar(BarCoverage::ProviderDay),
                match get("market_scope")? {
                    "consolidated" => MarketScope::Consolidated,
                    "xnys" => MarketScope::Venue(Venue::Xnys),
                    "xnas" => MarketScope::Venue(Venue::Xnas),
                    "arcx" => MarketScope::Venue(Venue::Arcx),
                    _ => return Err(Error::new(Kind::Invalid)),
                },
                SemanticReference::new(
                    ReferenceId::new(id("semantics_id")?).map_err(convert)?,
                    ReferenceRevision::new(
                        get("semantics_revision")?
                            .parse()
                            .map_err(|_| Error::new(Kind::Invalid))?,
                    )
                    .map_err(convert)?,
                ),
                match get("precision")? {
                    "date" if get("format")? == "edgeagent.admission.date-only.v1" => {
                        TimestampPrecision::DateOnly
                    }
                    "ms" => TimestampPrecision::Milliseconds,
                    "us" => TimestampPrecision::Microseconds,
                    "ns" => TimestampPrecision::Nanoseconds,
                    _ => return Err(Error::new(Kind::Invalid)),
                },
            )
            .map_err(|_| Error::new(Kind::Invalid))?,
            knowledge: if get("knowledge")? == "capture" {
                KnowledgeMode::Capture
            } else {
                KnowledgeMode::RetainedAt(at("knowledge")?)
            },
            admitted_at: at("admitted_ms")?,
            deadline: at("deadline_ms")?,
            raw: match get("raw")? {
                "retain" => RawRetention::Retain,
                "discard" => RawRetention::Discard,
                _ => return Err(Error::new(Kind::Invalid)),
            },
            budget: RequestBudget {
                connect_ms: positive("connect_ms")?,
                request_ms: positive("request_ms")?,
                calls: positive("calls")?,
                pages: positive("pages")?,
                response_bytes: positive("response_bytes")?,
                command_bytes: positive("command_bytes")?,
            },
        };
        if (get("format")? == "edgeagent.admission.date-only.v1")
            != (request.contract.rules().1 == TimestampPrecision::DateOnly)
        {
            return Err(Error::new(Kind::Invalid));
        }
        request
            .validate(request.admitted_at, &Cancellation::default())
            .map_err(|_| Error::new(Kind::Invalid))?;
        let limits = AcquisitionLimits {
            attempts: u8::try_from(n("attempts")?).map_err(|_| Error::new(Kind::Invalid))?,
            spacing_ms: n("spacing_ms")?,
            starts_per_minute: u8::try_from(n("starts_per_minute")?)
                .map_err(|_| Error::new(Kind::Invalid))?,
        };
        limits.validate()?;
        // Reconstruct through the same sealing path with authenticated catalogs later.
        // Canonical exact-byte comparison rejects duplicate fields and alternate encodings.
        validate_fields(&m)?;
        let record = Self::seal(request, limits, m)?;
        if record.command != *expected || record.bytes != bytes {
            return Err(Error::new(Kind::Invalid));
        }
        Ok(record)
    }
}

fn validate_fields(m: &BTreeMap<String, String>) -> Result<(), Error> {
    let ids = [
        "request",
        "principal",
        "account",
        "source",
        "instrument",
        "endpoint",
        "audience",
        "storage",
        "listing",
        "semantics_id",
    ];
    let integers = [
        "semantics_revision",
        "admitted_ms",
        "deadline_ms",
        "delete_ms",
        "connect_ms",
        "request_ms",
        "calls",
        "pages",
        "response_bytes",
        "command_bytes",
        "attempts",
        "spacing_ms",
        "starts_per_minute",
        "quality_revision",
        "reference_age_ms",
        "close_change_bps",
        "source_difference_bps",
    ];
    let rest = [
        "format",
        "precision",
        "market_scope",
        "start",
        "end",
        "knowledge",
        "raw",
        "rights",
        "review",
    ];
    let bound = m.get("format").is_some_and(|v| {
        v == "edgeagent.admission.daily.v2"
            || (v == "edgeagent.admission.date-only.v1" && m.contains_key("configuration_sha256"))
    });
    if bound {
        edgeagent_contracts::evidence::EvidenceDigest::new(
            m.get("configuration_sha256")
                .ok_or_else(|| Error::new(Kind::Invalid))?,
        )
        .map_err(|_| Error::new(Kind::Invalid))?;
    }
    if m.len() != ids.len() + integers.len() + rest.len() + usize::from(bound)
        || m.keys().any(|k| {
            !ids.contains(&k.as_str())
                && !integers.contains(&k.as_str())
                && !rest.contains(&k.as_str())
                && !(bound && k == "configuration_sha256")
        })
    {
        return Err(Error::new(Kind::Invalid));
    }
    let get = |k: &str| m.get(k).ok_or_else(|| Error::new(Kind::Invalid));
    for key in ids {
        EvidenceId::new(get(key)?).map_err(|_| Error::new(Kind::Invalid))?;
    }
    for key in integers {
        let value = get(key)?;
        let canonical = if key.ends_with("revision") {
            value.parse::<u64>().map(|n| n.to_string())
        } else {
            value.parse::<i64>().map(|n| n.to_string())
        }
        .map_err(|_| Error::new(Kind::Invalid))?;
        if &canonical != value {
            return Err(Error::new(Kind::Invalid));
        }
    }
    for key in ["start", "end"] {
        edgeagent_contracts::evidence::EvidenceDate::new(get(key)?)
            .map_err(|_| Error::new(Kind::Invalid))?;
    }
    let knowledge = get("knowledge")?;
    if knowledge != "capture"
        && knowledge
            .parse::<i64>()
            .map_err(|_| Error::new(Kind::Invalid))?
            .to_string()
            != *knowledge
    {
        return Err(Error::new(Kind::Invalid));
    }
    for key in ["rights", "review"] {
        let parts = get(key)?.split(':').collect::<Vec<_>>();
        let [id, revision, source, known, hash] = parts.as_slice() else {
            return Err(Error::new(Kind::Invalid));
        };
        EvidenceId::new(id).map_err(|_| Error::new(Kind::Invalid))?;
        EvidenceId::new(source).map_err(|_| Error::new(Kind::Invalid))?;
        edgeagent_contracts::evidence::EvidenceRevision::new(revision)
            .map_err(|_| Error::new(Kind::Invalid))?;
        let value = known
            .parse::<i64>()
            .map_err(|_| Error::new(Kind::Invalid))?;
        if value.to_string() != *known {
            return Err(Error::new(Kind::Invalid));
        }
        UtcInstant::from_unix_millis(value).map_err(|_| Error::new(Kind::Invalid))?;
        edgeagent_contracts::evidence::EvidenceDigest::new(hash)
            .map_err(|_| Error::new(Kind::Invalid))?;
    }
    let getn = |key| {
        get(key)?
            .parse::<u64>()
            .map_err(|_| Error::new(Kind::Invalid))
    };
    TransformationRevision::new(getn("quality_revision")?)
        .map_err(|_| Error::new(Kind::Invalid))?;
    DailyQualityPolicy::new(
        TransformationRevision::new(1).map_err(|_| Error::new(Kind::Invalid))?,
        getn("reference_age_ms")?,
        u16::try_from(getn("close_change_bps")?).map_err(|_| Error::new(Kind::Invalid))?,
        u16::try_from(getn("source_difference_bps")?).map_err(|_| Error::new(Kind::Invalid))?,
    )
    .map_err(|_| Error::new(Kind::Invalid))?;
    let delete = get("delete_ms")?
        .parse::<i64>()
        .map_err(|_| Error::new(Kind::Invalid))?;
    UtcInstant::from_unix_millis(delete).map_err(|_| Error::new(Kind::Invalid))?;
    let admitted = get("admitted_ms")?
        .parse::<i64>()
        .map_err(|_| Error::new(Kind::Invalid))?;
    let deadline = get("deadline_ms")?
        .parse::<i64>()
        .map_err(|_| Error::new(Kind::Invalid))?;
    if delete <= admitted || delete > deadline.saturating_add(86_400_000) {
        return Err(Error::new(Kind::Invalid));
    }
    Ok(())
}
