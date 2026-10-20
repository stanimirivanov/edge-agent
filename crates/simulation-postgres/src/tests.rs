use super::*;
use std::error::Error;
#[test]
fn published_identity_is_stable_and_scoped_even_when_accounts_share_a_manifest()
-> Result<(), Box<dyn Error>> {
    use edgeagent_simulation_domain::{ArtifactDigest, risk::RiskAccountId};
    let first = RiskAccountId::new(1)?;
    let second = RiskAccountId::new(2)?;
    let run = ArtifactDigest::of(b"same-frozen-manifest");
    let identity = context::event_identity(first, run, [1; 32]);
    assert_eq!(identity, context::event_identity(first, run, [1; 32]));
    assert_ne!(identity, context::event_identity(second, run, [1; 32]));
    assert_ne!(
        identity,
        context::event_identity(first, ArtifactDigest::of(b"another-run"), [1; 32])
    );
    assert_ne!(identity, context::event_identity(first, run, [2; 32]));
    assert_eq!(identity.len(), 64);
    Ok(())
}
#[test]
fn storage_invariants_do_not_retry_and_diagnostics_preserve_redacted_sources() {
    for error in [
        sqlx::Error::RowNotFound,
        sqlx::Error::ColumnNotFound("private financial field".into()),
    ] {
        let failure = storage(error);
        assert_eq!(failure.kind(), HandlerFailureKind::Permanent);
        assert!(failure.source().is_some());
        assert!(!format!("{failure:?} {failure}").contains("private financial"));
    }
    assert_eq!(
        storage(sqlx::Error::PoolClosed).kind(),
        HandlerFailureKind::Transient
    );
    assert_eq!(hex(&[0, 15, 16, 255]), "000f10ff");
}
