use crate::{HttpError, HttpTransport, HttpUsage, JsonDocument, transport::HttpSession};
use edgeagent_contracts::evidence::EvidenceId;
use edgeagent_market_data_application::{
    AcquisitionLedger, AcquisitionUsage, AdmissionRecord, BudgetError, BudgetErrorKind,
    CallCompletion, CallPermit, Cancellation, DispatchAuthority,
};

/// Redacted dispatch failure; explicit source inspection remains available for diagnosis.
#[derive(Debug)]
#[non_exhaustive]
pub enum DispatchError {
    /// Durable accounting or current authority rejected the operation.
    Budget(BudgetError),
    /// Bounded transport rejected the operation.
    Http(HttpError),
}
impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Budget(e) => e.fmt(f),
            Self::Http(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for DispatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Budget(e) => e,
            Self::Http(e) => e,
        })
    }
}
impl From<BudgetError> for DispatchError {
    fn from(e: BudgetError) -> Self {
        Self::Budget(e)
    }
}
impl From<HttpError> for DispatchError {
    fn from(e: HttpError) -> Self {
        Self::Http(e)
    }
}
/// Identifier-free point-in-time accounting, not a replacement for the authoritative ledger.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchUsage {
    /// Confirmed durable counters; outstanding reservations remain charged.
    pub durable: AcquisitionUsage,
    /// Local transport observations; not durable and not additive across replicas.
    pub local: HttpUsage,
    /// This local session cannot start another page until reconciliation/recovery.
    pub awaiting_confirmation: bool,
}
enum State {
    Idle,
    // Set before awaiting reservation: a lost reply cannot authorize a second dispatch.
    Blocked,
    Pending {
        permit: CallPermit,
        completed: Option<(CallCompletion, Result<JsonDocument, DispatchError>)>,
    },
}
/// Single-owner page dispatch. No request can bypass a newly committed call permit.
/// Dropping a dispatch future retains uncertainty; dropping a confirmation future retains
/// the completed page for `confirm_page`. Neither operation refunds an unknown request.
pub struct MeteredSession<'a, L, A> {
    ledger: &'a L,
    authority: &'a A,
    record: AdmissionRecord,
    http: HttpSession<'a>,
    state: State,
}
impl<L, A> std::fmt::Debug for MeteredSession<'_, L, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MeteredSession(..)")
    }
}
impl HttpTransport {
    /// Open a guarded session using an authenticated immutable admission and fresh local checks.
    /// # Errors
    /// Rejects missing/changed durable admission, revoked authority or expired original bounds.
    pub async fn start_metered<'a, L: AcquisitionLedger, A: DispatchAuthority>(
        &'a self,
        ledger: &'a L,
        record: &AdmissionRecord,
        authority: &'a A,
        cancellation: &Cancellation,
    ) -> Result<MeteredSession<'a, L, A>, DispatchError> {
        let (stored, _) = ledger.load(record.command()).await?;
        if stored != *record {
            return Err(denied(BudgetErrorKind::Conflict));
        }
        let check = authority.check(record, cancellation)?;
        if !check.matches(record) {
            return Err(denied(BudgetErrorKind::Denied));
        }
        let http = self.start_checked(record.request(), check.at(), check.valid_until_ms())?;
        Ok(MeteredSession {
            ledger,
            authority,
            record: stored,
            http,
            state: State::Idle,
        })
    }
}
fn denied(kind: BudgetErrorKind) -> DispatchError {
    BudgetError::new(kind).into()
}
impl<L: AcquisitionLedger, A: DispatchAuthority> MeteredSession<'_, L, A> {
    fn check(&mut self, cancel: &Cancellation) -> Result<(), DispatchError> {
        if cancel.is_cancelled() {
            return Err(denied(BudgetErrorKind::Cancelled));
        }
        let check = self.authority.check(&self.record, cancel)?;
        if !check.matches(&self.record) {
            return Err(denied(BudgetErrorKind::Denied));
        }
        self.http.recheck(&check)?;
        Ok(())
    }
    /// Dispatch one page, charging a fresh permit before I/O and confirming usage before release.
    /// `traversal` is the stable owner-selected pagination identity, not a fresh ID per page.
    /// No automatic retries, sleeps, pagination or recovered-permit dispatch occurs.
    /// # Errors
    /// An uncertain reservation/request stays fenced. A known completed page can retry only
    /// `confirm_page`; it cannot issue another GET while accounting is unconfirmed.
    pub async fn get_page(
        &mut self,
        traversal: &EvidenceId,
        next: Option<&str>,
        cancel: &Cancellation,
    ) -> Result<JsonDocument, DispatchError> {
        if !matches!(self.state, State::Idle) {
            return Err(denied(BudgetErrorKind::Busy));
        }
        self.check(cancel)?;
        self.http.validate_page(next)?;
        self.state = State::Blocked;
        let permit = match self
            .ledger
            .start_call(self.record.command(), Some(traversal))
            .await
        {
            Ok(p) => p,
            Err(e) => {
                if e.kind() != BudgetErrorKind::Storage {
                    self.state = State::Idle;
                }
                return Err(e.into());
            }
        };
        if permit.command() != self.record.command() {
            return Err(denied(BudgetErrorKind::Conflict));
        }
        self.state = State::Pending {
            permit,
            completed: None,
        };
        metrics::counter!("edgeagent_provider_dispatch_total", "stage" => "reserved").increment(1);
        let before = self.http.usage();
        let result = match self.check(cancel) {
            Ok(()) => self
                .http
                .get(next, cancel)
                .await
                .map_err(DispatchError::from),
            Err(e) => Err(e),
        };
        let after = self.http.usage();
        let bytes = after.observed_bytes.saturating_sub(before.observed_bytes);
        metrics::counter!("edgeagent_provider_observed_bytes_total").increment(bytes);
        // Partial/oversized responses and dropped futures keep the full reservation.
        // Only complete receipt or a proven pre-I/O rejection can release its byte remainder.
        if after.attempts == before.attempts || after.response_observed {
            let decoded_bytes =
                u32::try_from(bytes).map_err(|_| denied(BudgetErrorKind::Invalid))?;
            if let State::Pending { completed, .. } = &mut self.state {
                *completed = Some((
                    CallCompletion::Observed {
                        decoded_bytes,
                        retry_after_ms: if after.attempts == before.attempts {
                            None
                        } else {
                            after.retry_after_ms
                        },
                    },
                    result,
                ));
            }
            self.confirm_page(cancel).await
        } else {
            metrics::counter!("edgeagent_provider_dispatch_total", "stage" => "uncertain")
                .increment(1);
            result
        }
    }
    /// Retry idempotent accounting of an already received page, never another HTTP request.
    /// Reconciliation remains permitted after cancellation/expiry, but data release does not.
    /// # Errors
    /// Unknown work cannot be reconciled here. Storage errors/drop preserve the completed page;
    /// changed authority, cancellation or deadline discard it after accounting confirms.
    pub async fn confirm_page(
        &mut self,
        cancel: &Cancellation,
    ) -> Result<JsonDocument, DispatchError> {
        let State::Pending {
            permit,
            completed: Some((completion, _)),
        } = &self.state
        else {
            return Err(denied(BudgetErrorKind::Busy));
        };
        self.ledger.complete_call(permit, *completion).await?;
        metrics::counter!("edgeagent_provider_dispatch_total", "stage" => "confirmed").increment(1);
        let state = std::mem::replace(&mut self.state, State::Idle);
        self.check(cancel)?;
        if let State::Pending {
            completed: Some((_, result)),
            ..
        } = state
        {
            result
        } else {
            Err(denied(BudgetErrorKind::Invalid))
        }
    }
    /// Read confirmed ledger usage without exposing request, account, URL, payload or permit IDs.
    /// # Errors
    /// Rejects missing/changed admission and storage failures; never guesses durable counters.
    pub async fn usage(&self) -> Result<DispatchUsage, DispatchError> {
        let (record, snapshot) = self.ledger.load(self.record.command()).await?;
        if record != self.record {
            return Err(denied(BudgetErrorKind::Conflict));
        }
        Ok(DispatchUsage {
            durable: snapshot.usage(),
            local: self.http.usage(),
            awaiting_confirmation: !matches!(self.state, State::Idle),
        })
    }
}
