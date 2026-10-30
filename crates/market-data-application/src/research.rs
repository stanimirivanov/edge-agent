//! Match research intent to an owner-installed offline acquisition; never resolve URLs/rights.
use crate::AdmissionRecord;
use edgeagent_contracts::research::{AbstentionReason, ResearchRecord, ResearchRequest};
/// Check the pinned recipe's historical universe, full lookback and owner knowledge cutoff.
/// Session count comes from the restored owner recipe, not client calendar arithmetic.
/// # Errors
/// Returns a stable abstention when the installed recipe cannot satisfy the request.
pub fn select(
    request: &ResearchRequest,
    record: &AdmissionRecord,
    sessions: u16,
) -> Result<(), AbstentionReason> {
    ResearchRecord::Request(request.clone())
        .canonical_bytes()
        .map_err(|_| AbstentionReason::InvalidEvidence)?;
    if request.instruments != [format!("{:032x}", record.request().scope.instrument.get())] {
        return Err(AbstentionReason::UnsupportedUniverse);
    }
    if request.lookback_sessions >= sessions {
        return Err(AbstentionReason::InsufficientHistory);
    }
    if request.decision_at_ms < record.request().admitted_at.unix_millis() {
        return Err(AbstentionReason::FutureEvidence);
    }
    Ok(())
}
