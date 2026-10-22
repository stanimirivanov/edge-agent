use super::*;
use crate::AuthorizedAcquisition;
use std::future::Future;

/// Owner-private durable budget port. Calls return only after database commit.
/// A returned permit must accompany fresh `AuthorizedAcquisition` at dispatch;
/// it is not a substitute for current rights, HTTP deadlines or local worker slots.
pub trait AcquisitionLedger: Send + Sync {
    /// Admit immutable bytes under current checked authority, or recover identical admission.
    fn admit(
        &self,
        record: &AdmissionRecord,
        authority: &AuthorizedAcquisition<'_>,
    ) -> impl Future<Output = Result<AdmissionRecord>> + Send;
    /// Recover authenticated immutable admission and current durable counters.
    fn load(
        &self,
        command: &AcquireEvidence,
    ) -> impl Future<Output = Result<(AdmissionRecord, BudgetSnapshot)>> + Send;
    /// Charge an acquisition attempt.
    fn begin_attempt(
        &self,
        command: &AcquireEvidence,
        attempt: u8,
    ) -> impl Future<Output = Result<BudgetSnapshot>> + Send;
    /// Persist call/byte charges and the exclusive account permit before dispatch.
    fn start_call(
        &self,
        command: &AcquireEvidence,
        traversal: Option<&EvidenceId>,
    ) -> impl Future<Output = Result<CallPermit>> + Send;
    /// Reconcile exact completed/quiescent work; never call solely because a lease expired.
    fn complete_call(
        &self,
        permit: &CallPermit,
        outcome: CallCompletion,
    ) -> impl Future<Output = Result<BudgetSnapshot>> + Send;
    /// Cancel future work while retaining any unresolved call and charge.
    fn cancel(
        &self,
        command: &AcquireEvidence,
    ) -> impl Future<Output = Result<BudgetSnapshot>> + Send;
}
