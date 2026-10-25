//! Canonical owner-private precise v1/v2 and native-date admission. Digests are not authority.
use super::budget::{BudgetError as Error, BudgetErrorKind as Kind};
use crate::*;
use edgeagent_contracts::evidence::{AcquireEvidence, DailyEvidenceArtifact, EvidenceId};
use edgeagent_market_data_domain::*;
use std::collections::BTreeMap;
mod codec;

/// Explicit lower acquisition/account limits; no provider-plan guesses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcquisitionLimits {
    /// Positive maximum acquisition attempts, at most three.
    pub attempts: u8,
    /// Minimum account request-start spacing, 15000..=300000 milliseconds.
    pub spacing_ms: u32,
    /// Positive starts per rolling minute, at most four.
    pub starts_per_minute: u8,
}
impl AcquisitionLimits {
    pub(crate) fn validate(self) -> Result<(), Error> {
        if !(1..=3).contains(&self.attempts)
            || !(15000..=300000).contains(&self.spacing_ms)
            || !(1..=4).contains(&self.starts_per_minute)
        {
            return Err(Error::new(Kind::Invalid));
        }
        Ok(())
    }
}
/// Sealed complete acquisition inputs and pinned authority; not a retained read grant.
/// JSON is a lexically ordered flat map of exact strings, followed by one LF.
#[derive(Clone, Eq, PartialEq)]
pub struct AdmissionRecord {
    request: AcquisitionRequest,
    limits: AcquisitionLimits,
    fields: BTreeMap<String, String>,
    bytes: Vec<u8>,
    command: AcquireEvidence,
}
impl std::fmt::Debug for AdmissionRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdmissionRecord").finish_non_exhaustive()
    }
}
impl AdmissionRecord {
    /// Pin the complete owner-selected configuration before durable admission.
    /// V2 adds this digest to the immutable command identity; v1 remains readable.
    /// # Errors
    /// Rejects repinning an already bound record or an oversized encoding.
    pub fn with_configuration(
        self,
        digest: &edgeagent_contracts::evidence::EvidenceDigest,
    ) -> Result<Self, Error> {
        if self.fields.contains_key("configuration_sha256") {
            return Err(Error::new(Kind::Conflict));
        }
        let mut fields = self.fields;
        if self.request.contract.rules().1 != TimestampPrecision::DateOnly {
            fields.insert("format".into(), "edgeagent.admission.daily.v2".into());
        }
        fields.insert("configuration_sha256".into(), digest.as_str().into());
        Self::seal(self.request, self.limits, fields)
    }

    /// Owner-selected configuration pin, absent on legacy unbound admissions.
    #[must_use]
    pub fn configuration(&self) -> Option<&str> {
        self.fields.get("configuration_sha256").map(String::as_str)
    }

    /// Seal explicit request, listing, quality and reviewed authority before persistence.
    /// # Errors
    /// Rejects invalid limits, authority bindings or an over-8-KiB encoding.
    pub fn new(
        admitted: &AuthorizedAcquisition<'_>,
        limits: AcquisitionLimits,
        listing: ListingId,
        quality: DailyQualityPolicy,
    ) -> Result<Self, Error> {
        let r = *admitted.request();
        r.validate(r.admitted_at, &Cancellation::default())
            .map_err(|_| Error::new(Kind::Invalid))?;
        limits.validate()?;
        let mut m = BTreeMap::new();
        macro_rules! field {
            ($key:literal, $value:expr) => {
                m.insert($key.to_owned(), $value.to_string());
            };
        }
        macro_rules! id {
            ($key:literal, $value:expr) => {
                field!($key, format!("{:032x}", $value.get()));
            };
        }
        field!("format", "edgeagent.admission.daily.v1");
        if r.contract.rules().1 == TimestampPrecision::DateOnly {
            field!("format", "edgeagent.admission.date-only.v1");
        }
        id!("request", r.id);
        id!("principal", r.scope.principal);
        id!("account", r.scope.account);
        id!("source", r.scope.source);
        id!("instrument", r.scope.instrument);
        id!("endpoint", r.scope.endpoint);
        id!("audience", r.scope.audience);
        id!("storage", r.scope.storage);
        id!("listing", listing);
        let (semantics, precision) = r.contract.rules();
        id!("semantics_id", semantics.version().0);
        field!("semantics_revision", semantics.version().1.get());
        field!(
            "precision",
            match precision {
                TimestampPrecision::DateOnly => "date",
                TimestampPrecision::Milliseconds => "ms",
                TimestampPrecision::Microseconds => "us",
                TimestampPrecision::Nanoseconds => "ns",
                _ => return Err(Error::new(Kind::Invalid)),
            }
        );
        field!(
            "market_scope",
            match r.contract.shape().1 {
                MarketScope::Consolidated => "consolidated",
                MarketScope::Venue(Venue::Xnys) => "xnys",
                MarketScope::Venue(Venue::Xnas) => "xnas",
                MarketScope::Venue(Venue::Arcx) => "arcx",
                _ => return Err(Error::new(Kind::Invalid)),
            }
        );
        let (start, end) = r.range.bounds();
        for (key, date) in [("start", start), ("end", end)] {
            let (y, mn, d) = date.ymd();
            m.insert(key.into(), format!("{y:04}-{mn:02}-{d:02}"));
        }
        field!(
            "knowledge",
            match r.knowledge {
                KnowledgeMode::Capture => "capture".into(),
                KnowledgeMode::RetainedAt(at) => at.unix_millis().to_string(),
            }
        );
        field!("admitted_ms", r.admitted_at.unix_millis());
        field!("deadline_ms", r.deadline.unix_millis());
        field!("delete_ms", admitted.delete_by().unix_millis());
        field!(
            "raw",
            if r.raw == RawRetention::Retain {
                "retain"
            } else {
                "discard"
            }
        );
        field!("connect_ms", r.budget.connect_ms);
        field!("request_ms", r.budget.request_ms);
        field!("calls", r.budget.calls);
        field!("pages", r.budget.pages);
        field!("response_bytes", r.budget.response_bytes);
        field!("command_bytes", r.budget.command_bytes);
        field!("attempts", limits.attempts);
        field!("spacing_ms", limits.spacing_ms);
        field!("starts_per_minute", limits.starts_per_minute);
        let (revision, age, close, difference) = quality.parameters();
        field!("quality_revision", revision.get());
        field!("reference_age_ms", age);
        field!("close_change_bps", close);
        field!("source_difference_bps", difference);
        let (rights, review) = admitted.authority();
        for (prefix, v) in [("rights", pin(rights)), ("review", pin(review))] {
            m.insert(prefix.into(), v);
        }
        Self::seal(r, limits, m)
    }
    fn seal(
        request: AcquisitionRequest,
        limits: AcquisitionLimits,
        fields: BTreeMap<String, String>,
    ) -> Result<Self, Error> {
        let mut bytes = serde_json::to_vec(&fields).map_err(|_| Error::new(Kind::Invalid))?;
        bytes.push(b'\n');
        if bytes.len() > 8192 {
            return Err(Error::new(Kind::Invalid));
        }
        let command = AcquireEvidence {
            request: EvidenceId::new(&format!("{:032x}", request.id.get()))
                .map_err(|_| Error::new(Kind::Invalid))?,
            request_sha256: DailyEvidenceArtifact::hash(&bytes),
        };
        Ok(Self {
            request,
            limits,
            fields,
            bytes,
            command,
        })
    }
    /// Bound and authenticate retained bytes against their expected identity, then revalidate
    /// all supplied current authority and exact request/listing/quality inputs.
    /// # Errors
    /// Rejects different immutable inputs, noncanonical bytes, tampering or changed authority.
    pub fn verify(
        &self,
        admitted: &AuthorizedAcquisition<'_>,
        listing: ListingId,
        quality: DailyQualityPolicy,
    ) -> Result<(), Error> {
        let candidate = Self::new(admitted, self.limits, listing, quality)?;
        // A later current-rights check can shorten, but never extend, retained expiry.
        let mut fields = candidate.fields;
        if let Some(configuration) = self.configuration() {
            if self.request.contract.rules().1 != TimestampPrecision::DateOnly {
                fields.insert("format".into(), "edgeagent.admission.daily.v2".into());
            }
            fields.insert("configuration_sha256".into(), configuration.into());
        }
        fields.insert(
            "delete_ms".into(),
            self.fields
                .get("delete_ms")
                .ok_or_else(|| Error::new(Kind::Invalid))?
                .clone(),
        );
        if fields != self.fields || self.delete_by_ms()? > admitted.delete_by().unix_millis() {
            return Err(Error::new(Kind::Conflict));
        }
        Ok(())
    }
    /// Recheck all persisted request and authority bindings before durable admission.
    /// # Errors
    /// Rejects malformed inputs or changed current authority; grants no retained-use permission.
    pub fn verify_authority(&self, admitted: &AuthorizedAcquisition<'_>) -> Result<(), Error> {
        let get = |key: &str| {
            self.fields
                .get(key)
                .ok_or_else(|| Error::new(Kind::Invalid))
        };
        let listing = ListingId::new(
            u128::from_str_radix(get("listing")?, 16).map_err(|_| Error::new(Kind::Invalid))?,
        )
        .map_err(|_| Error::new(Kind::Invalid))?;
        let parse = |key| {
            get(key)?
                .parse::<u64>()
                .map_err(|_| Error::new(Kind::Invalid))
        };
        let quality = DailyQualityPolicy::new(
            TransformationRevision::new(parse("quality_revision")?)
                .map_err(|_| Error::new(Kind::Invalid))?,
            parse("reference_age_ms")?,
            u16::try_from(parse("close_change_bps")?).map_err(|_| Error::new(Kind::Invalid))?,
            u16::try_from(parse("source_difference_bps")?)
                .map_err(|_| Error::new(Kind::Invalid))?,
        )
        .map_err(|_| Error::new(Kind::Invalid))?;
        self.verify(admitted, listing, quality)
    }
    /// Exact immutable command reference, suitable for the identity-only bus contract.
    #[must_use]
    pub const fn command(&self) -> &AcquireEvidence {
        &self.command
    }
    /// Original request; changing remaining budgets requires a separate execution projection.
    #[must_use]
    pub const fn request(&self) -> &AcquisitionRequest {
        &self.request
    }
    /// Exact original listing; alias text cannot substitute for this identity.
    /// # Errors
    /// Rejects malformed retained listing metadata.
    pub fn listing(&self) -> Result<ListingId, Error> {
        let text = self
            .fields
            .get("listing")
            .ok_or_else(|| Error::new(Kind::Invalid))?;
        ListingId::new(u128::from_str_radix(text, 16).map_err(|_| Error::new(Kind::Invalid))?)
            .map_err(|_| Error::new(Kind::Invalid))
    }
    /// Exact configured lower limits.
    #[must_use]
    pub const fn limits(&self) -> AcquisitionLimits {
        self.limits
    }
    /// Canonical owner-private bytes; never log or publish their contents.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Exclusive first deadline, shortened by the original retention deadline.
    /// # Errors
    /// Returns Invalid for internally inconsistent retained metadata.
    pub fn valid_until_ms(&self) -> Result<i64, Error> {
        Ok(self
            .request
            .deadline
            .unix_millis()
            .min(self.delete_by_ms()?))
    }
    /// Original exclusive licensed deletion time; checks never renew this value.
    /// # Errors
    /// Returns Invalid for malformed retained metadata.
    pub fn delete_by_ms(&self) -> Result<i64, Error> {
        self.fields
            .get("delete_ms")
            .ok_or_else(|| Error::new(Kind::Invalid))?
            .parse()
            .map_err(|_| Error::new(Kind::Invalid))
    }
    /// Licensed cleanup namespace, independent of content hashes and rights revisions.
    /// # Errors
    /// Rejects malformed retained authority pins.
    pub fn retention_scope(&self) -> Result<crate::RetentionScope, Error> {
        let rights = self
            .fields
            .get("rights")
            .and_then(|p| p.split(':').next())
            .ok_or_else(|| Error::new(Kind::Invalid))?;
        let id = |s: &str| EvidenceId::new(s).map_err(|_| Error::new(Kind::Invalid));
        Ok(crate::RetentionScope {
            storage: id(&format!("{:032x}", self.request.scope.storage.get()))?,
            audience: id(&format!("{:032x}", self.request.scope.audience.get()))?,
            rights: id(rights)?,
        })
    }
    /// Bind an object upload to all original request/scope and authority identities.
    /// # Errors
    /// Denies foreign scope, changed rights pins or extended retention.
    pub fn verify_artifact(&self, artifact: &DailyEvidenceArtifact) -> Result<(), Error> {
        let data = artifact.data();
        self.verify_object(&data.scope, &data.references, data.delete_by)
    }
    /// Bind accepted native preparation to the original native admission and frozen selection.
    /// # Errors
    /// Denies cross-format, scope, configuration, range, rights or retention changes.
    pub fn verify_native_artifact(
        &self,
        artifact: &edgeagent_contracts::evidence::NativeDailyArtifact,
    ) -> Result<(), Error> {
        let data = artifact.data();
        let capture = &data.capture;
        let semantics = self.request.contract.rules().0.version();
        let captured = i128::from(data.captured_at.seconds) * 1000
            + i128::from(data.captured_at.nanos / 1_000_000);
        let quality_matches = [
            (
                "quality_revision",
                data.quality.revision.as_str().to_owned(),
            ),
            (
                "reference_age_ms",
                data.quality.reference_age_ms.as_str().to_owned(),
            ),
            (
                "close_change_bps",
                data.quality.close_change_bps.to_string(),
            ),
            (
                "source_difference_bps",
                data.quality.source_difference_bps.to_string(),
            ),
        ]
        .iter()
        .all(|(key, value)| self.fields.get(*key) == Some(value));
        if self.request.contract.rules().1 != TimestampPrecision::DateOnly
            || !quality_matches
            || capture.semantics.id.as_str() != format!("{:032x}", semantics.0.get())
            || capture.semantics.revision.as_str() != semantics.1.get().to_string()
            || captured < i128::from(self.request.admitted_at.unix_millis())
            || captured >= i128::from(self.valid_until_ms()?)
            || self.configuration() != Some(capture.selection_sha256.as_str())
            || capture.scope.listing.as_str() != format!("{:032x}", self.listing()?.get())
            || capture.range
                != [
                    crate::evidence::date(self.request.range.bounds().0)
                        .map_err(|_| Error::new(Kind::Invalid))?,
                    crate::evidence::date(self.request.range.bounds().1)
                        .map_err(|_| Error::new(Kind::Invalid))?,
                ]
        {
            return Err(Error::new(Kind::Denied));
        }
        self.verify_object(&capture.scope, &data.references, data.delete_by)
    }
    fn verify_object(
        &self,
        s: &edgeagent_contracts::evidence::EvidenceScope,
        references: &[edgeagent_contracts::evidence::EvidenceReference],
        delete_by: edgeagent_contracts::evidence::EvidenceTime,
    ) -> Result<(), Error> {
        let r = self.request.scope;
        let pairs = [
            (&s.principal, r.principal.get()),
            (&s.account, r.account.get()),
            (&s.source, r.source.get()),
            (&s.instrument, r.instrument.get()),
            (&s.endpoint, r.endpoint.get()),
            (&s.storage, r.storage.get()),
            (&s.audience, r.audience.get()),
        ];
        let rights = references
            .iter()
            .find(|p| p.kind == edgeagent_contracts::evidence::EvidenceReferenceKind::Rights)
            .ok_or_else(|| Error::new(Kind::Denied))?;
        let pin = self
            .fields
            .get("rights")
            .ok_or_else(|| Error::new(Kind::Invalid))?;
        let expected = format!(
            "{}:{}:",
            rights.version.id.as_str(),
            rights.version.revision.as_str()
        );
        let delete = i128::from(delete_by.seconds) * 1000 + i128::from(delete_by.nanos / 1_000_000);
        if s.request != self.command.request
            || s.request_sha256 != self.command.request_sha256
            || pairs
                .iter()
                .any(|(id, n)| id.as_str() != format!("{n:032x}"))
            || !pin.starts_with(&expected)
            || delete > i128::from(self.delete_by_ms()?)
        {
            return Err(Error::new(Kind::Denied));
        }
        Ok(())
    }
}
fn pin<T>(v: &ReferenceVersion<T>) -> String {
    let hash = v
        .checksum()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!(
        "{:032x}:{}:{:032x}:{}:{hash}",
        v.id().get(),
        v.revision().get(),
        v.source().get(),
        v.known_at().unix_millis()
    )
}
