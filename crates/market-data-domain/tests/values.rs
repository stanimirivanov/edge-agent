use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use edgeagent_market_data_domain::*;

#[test]
fn identities_are_nonzero_and_preserve_opaque_values() -> Result<(), ValueError> {
    assert_eq!(InstrumentId::new(0), Err(ValueError::ZeroIdentity));
    assert_eq!(ListingId::new(0), Err(ValueError::ZeroIdentity));
    assert_eq!(SourceId::new(0), Err(ValueError::ZeroIdentity));
    assert_eq!(
        TransformationRevision::new(0),
        Err(ValueError::ZeroIdentity)
    );
    assert_eq!(InstrumentId::new(u128::MAX)?.get(), u128::MAX);
    assert_eq!(ListingId::new(1)?.get(), 1);
    assert_eq!(SourceId::new(2)?.get(), 2);
    assert_eq!(TransformationRevision::new(u64::MAX)?.get(), u64::MAX);
    Ok(())
}

#[test]
fn exact_values_are_canonical_ordered_and_hash_equivalent() -> Result<(), ValueError> {
    let hash = |value: UsdPrice| {
        let mut h = DefaultHasher::new();
        value.hash(&mut h);
        h.finish()
    };
    let a = UsdPrice::new(123, 2)?;
    let b = UsdPrice::new(123000, 5)?;
    assert_eq!(a, b);
    assert_eq!(hash(a), hash(b));
    assert_eq!(a.decimal_parts(), (123, 2));
    assert!(UsdPrice::new(9, 1)? < UsdPrice::new(1, 0)?);
    assert!(ShareVolume::new(99, 2)? < ShareVolume::new(1, 0)?);
    assert_eq!(ShareVolume::new(0, 6)?.decimal_parts(), (0, 0));
    assert_eq!(a.currency(), Currency::Usd);
    assert_eq!(format!("{a:?}"), "UsdPrice(..)");
    assert_eq!(format!("{:?}", ShareVolume::new(42, 0)?), "ShareVolume(..)");
    Ok(())
}

#[test]
fn numeric_boundaries_never_round_or_overflow() -> Result<(), ValueError> {
    for value in [i128::MIN, -1, 0] {
        assert_eq!(UsdPrice::new(value, 0), Err(ValueError::InvalidSign));
    }
    assert_eq!(ShareVolume::new(-1, 0), Err(ValueError::InvalidSign));
    assert_eq!(UsdPrice::new(10, 9), Err(ValueError::Scale));
    assert_eq!(ShareVolume::new(0, u32::MAX), Err(ValueError::Scale));
    assert_eq!(ShareVolume::new(1, 7), Err(ValueError::Scale));
    assert_eq!(ShareVolume::new(1, 6)?.decimal_parts(), (1, 6));
    assert_eq!(ShareVolume::new(1, 7), Err(ValueError::Scale));
    assert_eq!(ShareVolume::new(1, 6)?.decimal_parts(), (1, 6));
    for value in [10_i128.pow(28), i128::MAX] {
        assert_eq!(UsdPrice::new(value, 0), Err(ValueError::Precision));
        assert_eq!(ShareVolume::new(value, 0), Err(ValueError::Precision));
    }
    for (coefficient, scale) in [
        (1, 8),
        (10_i128.pow(28) - 1, 0),
        (10_i128.pow(28) - 1, 8),
        (10_i128.pow(28), 1),
    ] {
        let value = UsdPrice::new(coefficient, scale)?;
        let (n, s) = value.decimal_parts();
        assert_eq!(UsdPrice::new(n, s)?, value);
    }
    let volume = ShareVolume::new(10_i128.pow(28) - 1, 0)?;
    let (n, s) = volume.decimal_parts();
    assert_eq!(ShareVolume::new(n, s)?, volume);
    Ok(())
}

#[test]
fn dates_use_gregorian_rules_without_claiming_market_membership() -> Result<(), ValueError> {
    assert_eq!(SessionDate::new(2000, 2, 29)?.ymd(), (2000, 2, 29));
    for (year, month, day) in [
        (1900, 2, 29),
        (2026, 2, 29),
        (0, 1, 1),
        (10000, 1, 1),
        (2026, 0, 1),
        (2026, 4, 31),
    ] {
        assert_eq!(SessionDate::new(year, month, day), Err(ValueError::Date));
    }
    assert!(SessionDate::new(1, 1, 1)? < SessionDate::new(9999, 12, 31)?);
    assert_eq!(UtcInstant::from_unix_millis(-1)?.unix_millis(), -1);
    assert!(UtcInstant::from_unix_millis(i64::MIN).is_err());
    assert!(UtcInstant::from_unix_millis(i64::MAX).is_err());
    for millis in [-62_135_596_800_000, 253_402_300_799_999] {
        assert_eq!(UtcInstant::from_unix_millis(millis)?.unix_millis(), millis);
    }
    assert!(UtcInstant::from_unix_millis(-62_135_596_800_001).is_err());
    assert!(UtcInstant::from_unix_millis(253_402_300_800_000).is_err());
    for millis in [-62_135_596_800_000, 253_402_300_799_999] {
        assert_eq!(UtcInstant::from_unix_millis(millis)?.unix_millis(), millis);
    }
    assert!(UtcInstant::from_unix_millis(-62_135_596_800_001).is_err());
    assert!(UtcInstant::from_unix_millis(253_402_300_800_000).is_err());
    Ok(())
}

#[test]
fn observation_times_preserve_unknowns_and_reject_causal_contradictions() -> Result<(), ValueError>
{
    let e = UtcInstant::from_unix_millis(0)?;
    let p = UtcInstant::from_unix_millis(1)?;
    let r = UtcInstant::from_unix_millis(2)?;
    let times = ObservationTimes::new(e, Some(p), Some(r), r)?;
    assert_eq!(
        (
            times.event_at(),
            times.provider_observed_at(),
            times.provider_published_at(),
            times.received_at()
        ),
        (e, Some(p), Some(r), r)
    );
    assert_eq!(
        ObservationTimes::new(e, None, None, r)?.provider_published_at(),
        None
    );
    for (event, observed, published, received) in [
        (r, None, None, e),
        (e, Some(r), Some(p), r),
        (e, Some(p), None, e),
        (e, None, Some(r), p),
        (p, Some(e), None, r),
        (p, None, Some(e), r),
        (p, Some(e), None, r),
        (p, None, Some(e), r),
    ] {
        assert_eq!(
            ObservationTimes::new(event, observed, published, received),
            Err(ValueError::TimeOrder)
        );
    }
    fn portable<T: Send + Sync + Copy + Eq + std::fmt::Debug>() {}
    portable::<UsdPrice>();
    portable::<ShareVolume>();
    portable::<ObservationTimes>();
    Ok(())
}
