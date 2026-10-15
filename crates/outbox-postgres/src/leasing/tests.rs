use super::{StoredClaim, claimed_message};
use crate::OutboxErrorKind;
use edgeagent_messaging::OutboxStoreError;
use std::error::Error;
use std::num::TryFromIntError;

const STORED_SENTINEL: &str = "private-stored-envelope-sentinel-6182";

fn stored_claim(attempt_count: i32, lease_generation: i64) -> StoredClaim {
    StoredClaim {
        message_source: "urn:edgeagent:test".to_owned(),
        message_id: "stored-claim".to_owned(),
        message_type: "dev.edgeagent.test.v1".to_owned(),
        transport_subject: "events.test.v1".to_owned(),
        envelope: STORED_SENTINEL.as_bytes().to_vec(),
        attempt_count,
        lease_generation,
    }
}

#[test]
fn invalid_stored_claims_preserve_conversion_and_validation_causes() -> Result<(), Box<dyn Error>> {
    let cases = [
        (
            stored_claim(1, -1),
            "stored lease_generation is outside the unsigned integer range",
            true,
        ),
        (
            stored_claim(1, 0),
            "stored lease_generation is not a positive claim generation",
            false,
        ),
        (
            stored_claim(-1, 1),
            "stored attempt_count is outside the unsigned integer range",
            true,
        ),
        (
            stored_claim(0, 1),
            "stored claim violates portable claim invariants",
            false,
        ),
    ];

    for (row, reason, is_conversion_failure) in cases {
        let error = claimed_message(row)
            .err()
            .ok_or("invalid stored claim must be rejected")?;
        assert_eq!(error.kind(), OutboxErrorKind::StorageInvariant);
        assert_eq!(
            error.to_string(),
            format!("outbox storage invariant failed: {reason}")
        );
        for diagnostic in [error.to_string(), format!("{error:?}")] {
            assert!(!diagnostic.contains(STORED_SENTINEL));
            assert!(!diagnostic.contains(&format!("{:?}", STORED_SENTINEL.as_bytes())));
        }
        let cause = error
            .source()
            .ok_or("stored conversion cause is required")?;
        if is_conversion_failure {
            assert!(cause.is::<TryFromIntError>());
        } else {
            assert!(cause.is::<OutboxStoreError>());
        }
    }
    Ok(())
}

#[test]
fn valid_stored_claim_preserves_attempt_generation_and_exact_bytes() -> Result<(), Box<dyn Error>> {
    let claim = claimed_message(stored_claim(2, 3))?;

    assert_eq!(claim.attempt(), 2);
    assert_eq!(claim.lease_generation().get(), 3);
    assert_eq!(claim.envelope_bytes(), STORED_SENTINEL.as_bytes());
    Ok(())
}
