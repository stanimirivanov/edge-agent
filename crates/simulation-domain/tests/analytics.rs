//! Independent numerical qualification, not provider/execution or universal accuracy.
#[path = "analytics/references.rs"]
mod references;
use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::{ArtifactDigest, analytics::*};
type Error = Box<dyn std::error::Error>;
fn time(ms: i64) -> Result<UtcInstant, Error> {
    Ok(UtcInstant::from_unix_millis(ms)?)
}
fn specification(
    kind: OptionKind,
    strike: f64,
    seconds: f64,
    exercise: ExerciseStyle,
    known: i64,
) -> Result<ReferenceVersion<OptionSpecification>, Error> {
    let expiry = time((seconds * 1000.0).round() as i64)?;
    Ok(ReferenceVersion::new(
        ReferenceId::new(1)?,
        ReferenceRevision::new(1)?,
        SourceId::new(1)?,
        time(known)?,
        [1; 32],
        OptionSpecification::new(
            InstrumentId::new(2)?,
            Underlying::Index(InstrumentId::new(1)?),
            OptionTerms::new(
                kind,
                OptionPoints::new((strike * 1e6).round() as i128, 6)?,
                OptionPoints::new(1, 2)?,
                UsdPerPoint::new(100, 0)?,
                exercise,
                SettlementStyle::Cash,
            )?,
            OptionSchedule::new(
                time(-1)?,
                expiry,
                expiry,
                expiry,
                expiry,
                OptionSchedule::timezone_rules().1,
            )?,
            EffectiveInterval::new(time(-1)?, time(expiry.unix_millis() + 1)?)?,
        )?,
    ))
}
fn input(p: [f64; 6], kind: OptionKind) -> Result<AnalyticsInputs, Error> {
    let [s, k, r, q, seconds, _] = p;
    Ok(AnalyticsInputs::new(
        specification(kind, k, seconds, ExerciseStyle::European, -1)?,
        PreciseInstant::from_millis(time(0)?),
        IndexPoints::new((s * 1e6).round() as i128, 6)?,
        CarryRates::new(r, q, ArtifactDigest::of(b"invented rates"))?,
        YearBasis::Actual365Fixed,
        ArtifactDigest::of(b"invented index evidence"),
    )?)
}
fn estimate(p: [f64; 6], kind: OptionKind) -> Result<EuropeanEstimate, Error> {
    Ok(input(p, kind)?.estimate(AnnualVolatility::new(p[5])?)?)
}
fn values(v: EuropeanEstimate) -> [f64; 6] {
    [
        v.premium_points(),
        v.delta(),
        v.gamma_per_point(),
        v.vega_per_unit_volatility(),
        v.theta_per_year(),
        v.rho_per_unit_rate(),
    ]
}
fn close(actual: f64, expected: f64, absolute: f64, relative: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= absolute + relative * expected.abs(),
        "actual={actual:?}, expected={expected:?}"
    );
}
fn policy(
    iterations: u16,
    price: f64,
    width: f64,
    vega: f64,
) -> Result<IvSolverPolicy, AnalyticsError> {
    IvSolverPolicy::new(
        AnnualVolatility::new(0.05)?,
        AnnualVolatility::new(1.0)?,
        price,
        width,
        vega,
        iterations,
    )
}
fn premium(v: f64) -> Result<OptionPoints, Error> {
    Ok(OptionPoints::new((v * 1e8).round() as i128, 8)?)
}

#[test]
fn independent_reference_premiums_and_greeks_cover_carry_and_intraday_expiry() -> Result<(), Error>
{
    for (p, expected) in references::CASES {
        for (kind, expected) in [OptionKind::Call, OptionKind::Put]
            .into_iter()
            .zip(expected)
        {
            for (actual, expected) in values(estimate(p, kind)?).into_iter().zip(expected) {
                close(actual, expected, 1e-8, 2e-11);
            }
        }
    }
    // Independent published MathWorks blsprice(100,95,.1,.25,.5) example:
    // https://www.mathworks.com/help/finance/blsprice.html (4-decimal precision).
    close(
        estimate([100., 95., 0.1, 0., 7884000., 0.5], OptionKind::Call)?.premium_points(),
        13.6953,
        0.00005,
        0.,
    );
    close(
        estimate([100., 95., 0.1, 0., 7884000., 0.5], OptionKind::Put)?.premium_points(),
        6.3497,
        0.00005,
        0.,
    );
    Ok(())
}
#[test]
fn analytic_greeks_satisfy_parity_and_independent_finite_differences() -> Result<(), Error> {
    for (p, _) in references::CASES {
        let call = estimate(p, OptionKind::Call)?;
        let put = estimate(p, OptionKind::Put)?;
        let t = p[4] / 31536000.;
        let discount = (-p[3] * t).exp();
        close(
            call.premium_points() - put.premium_points(),
            p[0] * discount - p[1] * (-p[2] * t).exp(),
            1e-8,
            1e-10,
        );
        close(call.delta() - put.delta(), discount, 1e-12, 1e-11);
        close(call.gamma_per_point(), put.gamma_per_point(), 1e-12, 1e-11);
        close(
            call.vega_per_unit_volatility(),
            put.vega_per_unit_volatility(),
            1e-12,
            1e-11,
        );
        for kind in [OptionKind::Call, OptionKind::Put] {
            let base = estimate(p, kind)?;
            let h = if p[4] < 100. { 0.0001 } else { 0.001 };
            let mut plus = p;
            let mut minus = p;
            plus[0] += h;
            minus[0] -= h;
            let up = estimate(plus, kind)?;
            let down = estimate(minus, kind)?;
            close(
                (up.premium_points() - down.premium_points()) / (2. * h),
                base.delta(),
                1e-7,
                1e-6,
            );
            close(
                (up.delta() - down.delta()) / (2. * h),
                base.gamma_per_point(),
                1e-8,
                1e-6,
            );
            for (index, expected) in [
                (5, base.vega_per_unit_volatility()),
                (2, base.rho_per_unit_rate()),
            ] {
                let mut plus = p;
                let mut minus = p;
                plus[index] += 1e-5;
                minus[index] -= 1e-5;
                close(
                    (estimate(plus, kind)?.premium_points()
                        - estimate(minus, kind)?.premium_points())
                        / 2e-5,
                    expected,
                    1e-6,
                    1e-5,
                );
            }
            let step = if p[4] < 100. { 0.001 } else { 1. };
            let mut plus = p;
            let mut minus = p;
            plus[4] += step;
            minus[4] -= step;
            close(
                -(estimate(plus, kind)?.premium_points() - estimate(minus, kind)?.premium_points())
                    / (2. * step / 31536000.),
                base.theta_per_year(),
                0.002,
                1e-6,
            );
        }
    }
    Ok(())
}
#[test]
fn solver_requires_price_and_width_convergence_and_never_returns_an_exhausted_iterate()
-> Result<(), Error> {
    for (p, _) in references::CASES {
        for kind in [OptionKind::Call, OptionKind::Put] {
            let input = input(p, kind)?;
            let target = premium(estimate(p, kind)?.premium_points())?;
            let result = input.implied_volatility(target, policy(128, 1e-10, 1e-10, 1e-8)?)?;
            let ((lo, hi), residual, iterations) = result.convergence();
            assert!(hi - lo <= 1e-10 && residual <= 1e-10 && iterations <= 128);
            assert!(result.volatility().get() >= lo && result.volatility().get() <= hi);
            close(result.volatility().get(), p[5], 1e-7, 0.);
            assert_eq!(result.estimate(), input.estimate(result.volatility())?);
            assert_eq!(
                input.implied_volatility(target, policy(1, 1e-10, 1e-12, 1e-8)?),
                Err(AnalyticsError::NonConvergence)
            );
            assert_eq!(
                input.implied_volatility(target, policy(128, 1e-10, 1e-10, 1e9)?),
                Err(AnalyticsError::Unidentifiable)
            );
        }
    }
    let i = input(references::CASES[0].0, OptionKind::Call)?;
    // At the very first midpoint the price residual passes, but the bracket
    // remains wide. This catches changing the two convergence criteria to OR.
    let midpoint_price = premium(i.estimate(AnnualVolatility::new(0.525)?)?.premium_points())?;
    assert_eq!(
        i.implied_volatility(midpoint_price, policy(1, 1e-8, 1e-12, 1e-8)?),
        Err(AnalyticsError::NonConvergence)
    );
    let narrow = IvSolverPolicy::new(
        AnnualVolatility::new(0.4)?,
        AnnualVolatility::new(0.5)?,
        1e-8,
        1e-8,
        1e-8,
        128,
    )?;
    assert_eq!(
        i.implied_volatility(premium(10.45058357)?, narrow),
        Err(AnalyticsError::NotBracketed)
    );
    for p in [0., 1., 100., 101.] {
        assert_eq!(
            i.implied_volatility(premium(p)?, policy(128, 1e-8, 1e-8, 1e-8)?),
            Err(AnalyticsError::PremiumBounds)
        );
    }
    assert_eq!(
        i.implied_volatility(premium(10.45058357)?, policy(256, 1e-300, 1e-300, 1e-8)?),
        Err(AnalyticsError::NonConvergence)
    );
    Ok(())
}
#[test]
fn near_expiry_and_flat_time_value_cannot_fabricate_identified_volatility() -> Result<(), Error> {
    let p = [5000., 5000., 0.03, 0.01, 60., 0.35];
    let reference = specification(OptionKind::Call, 5000., 60., ExerciseStyle::European, -1)?;
    let near = AnalyticsInputs::new(
        reference.clone(),
        PreciseInstant::from_unix_parts(59, 999_999_999)?,
        IndexPoints::new(5000, 0)?,
        CarryRates::new(0.03, 0.01, ArtifactDigest::of(b"rates"))?,
        YearBasis::Actual365Fixed,
        ArtifactDigest::of(b"index"),
    )?;
    let v = near.estimate(AnnualVolatility::new(0.35)?)?;
    assert!(values(v).iter().all(|n| n.is_finite()));
    assert!(v.gamma_per_point() > estimate(p, OptionKind::Call)?.gamma_per_point());
    assert_eq!(
        near.implied_volatility(premium(v.premium_points())?, policy(128, 1e-10, 1e-8, 0.1)?),
        Err(AnalyticsError::Unidentifiable)
    );
    assert_eq!(
        AnalyticsInputs::new(
            reference,
            PreciseInstant::from_millis(time(60000)?),
            IndexPoints::new(5000, 0)?,
            CarryRates::new(0., 0., ArtifactDigest::of(b"rates"))?,
            YearBasis::Actual365Fixed,
            ArtifactDigest::of(b"index")
        )
        .err(),
        Some(AnalyticsError::Expired)
    );
    assert_eq!(
        estimate([1., 1000000., 0., 0., 60., 0.01], OptionKind::Call)
            .err()
            .and_then(|e| e.downcast_ref::<AnalyticsError>().copied()),
        Some(AnalyticsError::Numerical)
    );
    Ok(())
}
#[test]
fn nonfinite_degenerate_unsupported_and_unknown_inputs_fail_closed() -> Result<(), Error> {
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0., -1., 10.01] {
        assert_eq!(AnnualVolatility::new(v), Err(AnalyticsError::Input));
    }
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.01, -1.01] {
        assert_eq!(
            CarryRates::new(v, 0., ArtifactDigest::of(b"rates")),
            Err(AnalyticsError::Input)
        );
        assert_eq!(
            CarryRates::new(0., v, ArtifactDigest::of(b"rates")),
            Err(AnalyticsError::Input)
        );
    }
    for (budget, price, width, vega) in [
        (0, 1e-8, 1e-8, 1e-8),
        (257, 1e-8, 1e-8, 1e-8),
        (1, 0., 1e-8, 1e-8),
        (1, 1e-8, f64::NAN, 1e-8),
        (1, 1e-8, 1e-8, f64::INFINITY),
    ] {
        assert!(policy(budget, price, width, vega).is_err());
    }
    let at = PreciseInstant::from_millis(time(0)?);
    let level = IndexPoints::new(100, 0)?;
    let rates = CarryRates::new(0., 0., ArtifactDigest::of(b"rates"))?;
    for (exercise, known, error) in [
        (ExerciseStyle::American, -1, AnalyticsError::Unsupported),
        (ExerciseStyle::European, 1, AnalyticsError::Reference),
    ] {
        assert_eq!(
            AnalyticsInputs::new(
                specification(OptionKind::Call, 100., 100., exercise, known)?,
                at,
                level,
                rates,
                YearBasis::Actual365Fixed,
                ArtifactDigest::of(b"index")
            )
            .err(),
            Some(error)
        );
    }
    let r = specification(OptionKind::Call, 100., 100., ExerciseStyle::European, -1)?;
    assert_eq!(
        AnalyticsInputs::new(
            r.clone(),
            PreciseInstant::from_millis(time(-2)?),
            level,
            rates,
            YearBasis::Actual365Fixed,
            ArtifactDigest::of(b"index")
        )
        .err(),
        Some(AnalyticsError::Reference)
    );
    let delayed_effective = ReferenceVersion::new(
        r.id(),
        r.revision(),
        r.source(),
        r.known_at(),
        *r.checksum(),
        OptionSpecification::new(
            r.value().id(),
            r.value().underlying(),
            r.value().terms(),
            r.value().schedule(),
            EffectiveInterval::new(time(1)?, time(100001)?)?,
        )?,
    );
    assert_eq!(
        AnalyticsInputs::new(
            delayed_effective,
            at,
            level,
            rates,
            YearBasis::Actual365Fixed,
            ArtifactDigest::of(b"index")
        )
        .err(),
        Some(AnalyticsError::Reference)
    );
    let original = r.value();
    for (underlying, settlement) in [
        (
            Underlying::Equity(InstrumentId::new(1)?),
            SettlementStyle::Cash,
        ),
        (
            original.underlying(),
            SettlementStyle::Physical(DeliverableReference::new(
                ReferenceId::new(2)?,
                ReferenceRevision::new(1)?,
            )),
        ),
    ] {
        let terms = OptionTerms::new(
            OptionKind::Call,
            OptionPoints::new(100, 0)?,
            OptionPoints::new(1, 2)?,
            UsdPerPoint::new(100, 0)?,
            ExerciseStyle::European,
            settlement,
        )?;
        let changed = ReferenceVersion::new(
            r.id(),
            r.revision(),
            r.source(),
            r.known_at(),
            *r.checksum(),
            OptionSpecification::new(
                original.id(),
                underlying,
                terms,
                original.schedule(),
                original.effective(),
            )?,
        );
        assert_eq!(
            AnalyticsInputs::new(
                changed,
                at,
                level,
                rates,
                YearBasis::Actual365Fixed,
                ArtifactDigest::of(b"index")
            )
            .err(),
            Some(AnalyticsError::Unsupported)
        );
    }
    assert_eq!(
        input([1e13, 100., 0., 0., 100., 0.2], OptionKind::Call)
            .err()
            .and_then(|e| e.downcast_ref::<AnalyticsError>().copied()),
        Some(AnalyticsError::Input)
    );
    Ok(())
}
#[test]
fn retained_identities_reproduce_results_and_diagnostics_redact_inputs() -> Result<(), Error> {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<AnalyticsInputs>();
    send_sync::<EuropeanEstimate>();
    send_sync::<ImpliedVolatility>();
    let p = references::CASES[0].0;
    let i = input(p, OptionKind::Call)?;
    let v = AnnualVolatility::new(0.2)?;
    assert_eq!(i.identity(), input(p, OptionKind::Call)?.identity());
    assert_eq!(i.estimate(v)?, input(p, OptionKind::Call)?.estimate(v)?);
    let target = premium(i.estimate(v)?.premium_points())?;
    let a = i.implied_volatility(target, policy(128, 1e-8, 1e-8, 1e-8)?)?;
    assert_eq!(
        a,
        i.implied_volatility(target, policy(128, 1e-8, 1e-8, 1e-8)?)?
    );
    assert_ne!(
        a.identity(),
        i.implied_volatility(target, policy(129, 1e-8, 1e-8, 1e-8)?)?
            .identity()
    );
    assert_ne!(
        i.estimate(v)?.identity(),
        i.estimate(AnnualVolatility::new(0.3)?)?.identity()
    );
    for index in 0..5 {
        let mut changed = p;
        changed[index] += if index == 4 { 1. } else { 0.01 };
        assert_ne!(i.identity(), input(changed, OptionKind::Call)?.identity());
    }
    let r = specification(OptionKind::Call, 100., p[4], ExerciseStyle::European, -1)?;
    for changed_reference in [
        ReferenceVersion::new(
            r.id(),
            ReferenceRevision::new(2)?,
            r.source(),
            r.known_at(),
            *r.checksum(),
            *r.value(),
        ),
        ReferenceVersion::new(
            r.id(),
            r.revision(),
            SourceId::new(2)?,
            r.known_at(),
            *r.checksum(),
            *r.value(),
        ),
        ReferenceVersion::new(
            r.id(),
            r.revision(),
            r.source(),
            time(0)?,
            *r.checksum(),
            *r.value(),
        ),
        ReferenceVersion::new(
            r.id(),
            r.revision(),
            r.source(),
            r.known_at(),
            [2; 32],
            *r.value(),
        ),
    ] {
        let changed = AnalyticsInputs::new(
            changed_reference,
            PreciseInstant::from_millis(time(0)?),
            IndexPoints::new(100, 0)?,
            CarryRates::new(0.05, 0., ArtifactDigest::of(b"invented rates"))?,
            YearBasis::Actual365Fixed,
            ArtifactDigest::of(b"invented index evidence"),
        )?;
        assert_ne!(i.identity(), changed.identity());
    }
    for (evidence, assumptions) in [
        (b"changed evidence".as_slice(), b"invented rates".as_slice()),
        (
            b"invented index evidence".as_slice(),
            b"changed assumptions".as_slice(),
        ),
    ] {
        let changed = AnalyticsInputs::new(
            r.clone(),
            PreciseInstant::from_millis(time(0)?),
            IndexPoints::new(100, 0)?,
            CarryRates::new(0.05, 0., ArtifactDigest::of(assumptions))?,
            YearBasis::Actual365Fixed,
            ArtifactDigest::of(evidence),
        )?;
        assert_ne!(i.identity(), changed.identity());
    }
    let signed_zero = CarryRates::new(-0., 0., ArtifactDigest::of(b"rates"))?;
    assert_eq!(
        signed_zero,
        CarryRates::new(0., -0., ArtifactDigest::of(b"rates"))?
    );
    assert_eq!(format!("{i:?}"), "AnalyticsInputs(..)");
    assert_eq!(format!("{:?}", i.estimate(v)?), "EuropeanEstimate(..)");
    assert_eq!(format!("{a:?}"), "ImpliedVolatility(..)");
    assert_eq!(
        format!("{:?}", policy(128, 1e-8, 1e-8, 1e-8)?),
        "IvSolverPolicy(..)"
    );
    Ok(())
}
