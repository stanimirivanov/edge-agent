//! Synthetic contracts; no wall clock, infrastructure, credentials or vendor calls.
#[allow(dead_code)]
#[path = "support/streaming.rs"]
mod fixture;
use edgeagent_contracts::evidence::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use fixture::*;

const AT: i64 = 1_800_000_000_000;
fn usage() -> StreamingUsage {
    StreamingUsage {
        starts: 1,
        frames: 0,
        bytes: 0,
        last_frame_bytes: None,
        stopped: false,
    }
}

#[test]
fn stream_admission_pins_complete_selection_and_current_authority() -> Result {
    let f = Fixture::new(AT, 2)?;
    let session = f.session(1)?;
    assert_eq!(session.bytes(), f.session(1)?.bytes());
    let mut changed = f.data(1)?;
    changed.configuration = DailyEvidenceArtifact::hash(b"changed");
    assert_ne!(session.bytes(), f.seal(changed)?.bytes());
    let mut changed = f.data(1)?;
    changed.scopes.pop();
    assert!(f.seal(changed).is_err());
    let mut changed = f.data(1)?;
    changed.scopes.reverse();
    assert!(f.seal(changed).is_err());
    let mut changed = f.data(1)?;
    changed.scopes.first_mut().ok_or("scope")?.account = ProviderAccountId::new(2)?;
    assert!(f.seal(changed).is_err());
    assert!(Fixture::new(AT, 0)?.session(1).is_err());
    assert!(!format!("{session:?}").contains("SYNTH"));
    Ok(())
}
#[test]
fn stream_windows_limits_and_delivery_never_inherit_daily_defaults() -> Result {
    let f = Fixture::new(AT, 1)?;
    let mut data = f.data(1)?;
    data.limits.connect_ms = nz(3_600_001)?;
    assert!(f.seal(data).is_err());
    let mut data = f.data(1)?;
    data.limits.bytes = std::num::NonZeroU64::new(9).ok_or("nonzero")?;
    assert!(f.seal(data).is_err());
    let mut data = f.data(1)?;
    data.limits.subscriptions = std::num::NonZeroU16::new(33).ok_or("nonzero")?;
    assert!(f.seal(data).is_err());
    let mut data = f.data(1)?;
    data.contract = EvidenceContract::new(
        EvidenceKind::RawBar(BarCoverage::ProviderDay),
        MarketScope::Consolidated,
        data.contract.rules().0,
        TimestampPrecision::Milliseconds,
    )?;
    assert!(f.seal(data).is_err());
    let session = f.session(1)?;
    session.recheck(f.catalogs(), at(AT)?, &Cancellation::default())?;
    assert_eq!(
        session
            .recheck(f.catalogs(), at(AT + 3_600_000)?, &Cancellation::default())
            .err()
            .ok_or("deadline")?
            .kind(),
        BudgetErrorKind::Deadline
    );
    let revoked = f.revoked()?;
    assert_eq!(
        session
            .recheck(
                AuthorityCatalogs {
                    rights: &revoked,
                    providers: &f.providers
                },
                at(AT + 1)?,
                &Cancellation::default()
            )
            .err()
            .ok_or("revoked")?
            .kind(),
        BudgetErrorKind::Denied
    );
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        session
            .recheck(f.catalogs(), at(AT)?, &cancel)
            .err()
            .ok_or("cancel")?
            .kind(),
        BudgetErrorKind::Cancelled
    );
    Ok(())
}
#[test]
fn stream_frame_receipts_charge_controls_and_stop_at_exact_limits() -> Result {
    let f = Fixture::new(AT, 1)?;
    let limits = f.data(1)?.limits;
    let first = limits.charge(usage(), 1, 0)?;
    assert_eq!(first.usage.frames, 1);
    assert_eq!(first.usage.bytes, 0);
    assert!(limits.charge(first.usage, 1, 0)?.repeated);
    assert!(limits.charge(first.usage, 1, 1).is_err());
    assert!(limits.charge(first.usage, 3, 1).is_err());
    let second = limits.charge(first.usage, 2, 10)?;
    assert!(!second.usage.stopped);
    let final_frame = limits.charge(second.usage, 3, 10)?;
    assert!(final_frame.within_limits);
    assert!(final_frame.usage.stopped);
    assert_eq!((final_frame.usage.frames, final_frame.usage.bytes), (3, 20));
    assert_eq!(
        limits.charge(final_frame.usage, 3, 10)?.usage,
        final_frame.usage
    );
    assert!(limits.charge(final_frame.usage, 4, 0).is_err());
    Ok(())
}
#[test]
fn stream_overages_and_invalid_state_cannot_refund_or_continue() -> Result {
    let limits = Fixture::new(AT, 1)?.data(1)?.limits;
    let overage = limits.charge(usage(), 1, 11)?;
    assert!(!overage.within_limits);
    assert!(overage.usage.stopped);
    assert_eq!(overage.usage.bytes, 11);
    assert_eq!(limits.charge(overage.usage, 1, 11)?.usage, overage.usage);
    assert!(limits.charge(overage.usage, 2, 0).is_err());
    assert!(
        limits
            .charge(
                StreamingUsage {
                    starts: 0,
                    ..usage()
                },
                1,
                0
            )
            .is_err()
    );
    assert!(
        limits
            .charge(
                StreamingUsage {
                    frames: 7,
                    last_frame_bytes: Some(0),
                    ..usage()
                },
                8,
                0
            )
            .is_err()
    );
    Ok(())
}
#[test]
fn stream_connection_fences_bind_session_account_and_preserve_start_snapshot() -> Result {
    let f = Fixture::new(AT, 1)?;
    let one = f.session(1)?;
    let two = f.session(2)?;
    assert!(StreamingConnection::from_committed(&one, 0, usage()).is_err());
    assert!(StreamingConnection::from_committed(&one, u64::MAX, usage()).is_err());
    let connection = StreamingConnection::from_committed(&one, 1, usage())?;
    assert!(connection.matches(&one));
    assert!(!connection.matches(&two));
    assert_eq!(connection.usage(), usage());
    assert_eq!(format!("{connection:?}"), "StreamingConnection(..)");
    Ok(())
}
