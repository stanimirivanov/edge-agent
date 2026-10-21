use super::*;
use edgeagent_contracts::audit::MAX_AUDIT_BYTES;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    version: String,
    capacity: usize,
    receipts: Vec<String>,
    genesis_cash: u64,
    genesis_cost: Option<u64>,
    latest: String,
}
impl AccountProjection {
    /// Complete versioned view and duplicate receipts plus corruption checksum.
    /// Persist atomically with consumer progress. Checksum is NOT authentication
    /// or protection against an authorized rollback; storage/owner fencing is external.
    ///
    /// # Errors
    /// Reject an oversized or unserializable continuation.
    pub fn checkpoint(&self) -> Result<Vec<u8>, ReplayError> {
        let checkpoint = Checkpoint {
            version: "EAAP1".into(),
            capacity: self.capacity,
            receipts: self.receipts.clone(),
            genesis_cash: self.genesis_cash,
            genesis_cost: self.genesis_cost,
            latest: String::from_utf8(self.latest.encode().map_err(|_| ReplayError::Invalid)?)
                .map_err(|_| ReplayError::Invalid)?,
        };
        let mut bytes = serde_json::to_vec(&checkpoint).map_err(|_| ReplayError::Invalid)?;
        bytes.extend(Sha256::digest(&bytes));
        if bytes.len() > 3 * MAX_AUDIT_BYTES {
            return Err(ReplayError::Capacity);
        }
        Ok(bytes)
    }
    /// Restore against the authenticated original genesis, rejecting wrong
    /// account/run/configuration/funding before exposing a view. Authenticate and
    /// fence checkpoint storage separately; this does not detect authorized rollback.
    ///
    /// # Errors
    /// Reject checksum, version, receipt census, latest-state binding or bounds.
    pub fn restore(genesis: &AccountAuditFrame, bytes: &[u8]) -> Result<Self, ReplayError> {
        if bytes.len() > 3 * MAX_AUDIT_BYTES {
            return Err(ReplayError::Capacity);
        }
        let split = bytes.len().checked_sub(32).ok_or(ReplayError::Invalid)?;
        let (body, checksum) = bytes.split_at_checked(split).ok_or(ReplayError::Invalid)?;
        if Sha256::digest(body).as_slice() != checksum {
            return Err(ReplayError::Invalid);
        }
        let c: Checkpoint = serde_json::from_slice(body).map_err(|_| ReplayError::Invalid)?;
        let latest =
            AccountAuditFrame::decode(c.latest.as_bytes()).map_err(|_| ReplayError::Invalid)?;
        if c.version != "EAAP1"
            || !(1..=8193).contains(&c.capacity)
            || c.receipts.len() > c.capacity
            || latest.state().revision as usize + 1 != c.receipts.len()
            || c.receipts.iter().any(|s| {
                s.len() != 64
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            || c.receipts.last() != Some(&fingerprint(&latest)?)
            || (c.receipts.len() > 1
                && latest.previous() != c.receipts.get(c.receipts.len() - 2).map(String::as_str))
            || i128::from(c.genesis_cash) + i128::from(latest.state().postings[0])
                != i128::from(latest.state().cash_cents)
        {
            return Err(ReplayError::Invalid);
        }
        if let Some(initial) = c.genesis_cost
            && cost(latest.state())?.map(i128::from)
                != Some(i128::from(initial) + i128::from(latest.state().postings[1]))
        {
            return Err(ReplayError::Invalid);
        }
        let baseline = Self::new(genesis.clone(), c.capacity)?;
        if !same_scope(latest.state(), baseline.state()) {
            return Err(ReplayError::Scope);
        }
        if c.genesis_cash != baseline.genesis_cash
            || c.genesis_cost != baseline.genesis_cost
            || c.receipts.first() != baseline.receipts.first()
        {
            return Err(ReplayError::Conflict);
        }
        Ok(Self {
            capacity: c.capacity,
            receipts: c.receipts,
            latest,
            genesis_cash: c.genesis_cash,
            genesis_cost: c.genesis_cost,
        })
    }
}
