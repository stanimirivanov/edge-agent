use edgeagent_market_data_domain::*;
use edgeagent_simulation_domain::{ArtifactDigest, execution::PositionCost, settlement::*};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn at(second: i64) -> TestResult<PreciseInstant> {
    Ok(PreciseInstant::from_unix_parts(second, 0)?)
}
fn utc(second: i64) -> TestResult<UtcInstant> {
    Ok(UtcInstant::from_unix_millis(second * 1000)?)
}
fn specification(
    kind: OptionKind,
    exercise: ExerciseStyle,
    settlement: SettlementStyle,
    underlying: Underlying,
) -> TestResult<ReferenceVersion<OptionSpecification>> {
    Ok(ReferenceVersion::new(
        ReferenceId::new(5)?,
        ReferenceRevision::new(1)?,
        SourceId::new(7)?,
        utc(0)?,
        [9; 32],
        OptionSpecification::new(
            InstrumentId::new(2)?,
            underlying,
            OptionTerms::new(
                kind,
                OptionPoints::new(100, 0)?,
                OptionPoints::new(1, 2)?,
                UsdPerPoint::new(100, 0)?,
                exercise,
                settlement,
            )?,
            OptionSchedule::new(
                utc(0)?,
                utc(7)?,
                utc(8)?,
                utc(9)?,
                utc(10)?,
                OptionSchedule::timezone_rules().1,
            )?,
            EffectiveInterval::new(utc(0)?, utc(15)?)?,
        )?,
    ))
}
fn policy(
    kind: OptionKind,
    rounding: SettlementRounding,
    charges: SettlementCharges,
) -> TestResult<SettlementPolicy> {
    Ok(SettlementPolicy::new(
        specification(
            kind,
            ExerciseStyle::European,
            SettlementStyle::Cash,
            Underlying::Index(InstrumentId::new(1)?),
        )?,
        SemanticReference::new(ReferenceId::new(8)?, ReferenceRevision::new(1)?),
        SourceId::new(9)?,
        at(8)?,
        at(14)?,
        rounding,
        charges,
    )?)
}
fn fixing(
    policy: &SettlementPolicy,
    coefficient: i128,
    scale: u32,
) -> TestResult<SyntheticFinalFixing> {
    Ok(SyntheticFinalFixing::new(
        policy,
        ObservationId::new(1)?,
        at(12)?,
        IndexPoints::new(coefficient, scale)?,
    )?)
}
fn calculated(result: SettlementDecision) -> TestResult<SettlementCalculation> {
    match result {
        SettlementDecision::Calculated(effect) => Ok(effect),
        _ => Err("unexpected deferral".into()),
    }
}

#[test]
fn cash_calls_and_puts_close_whole_position_with_balanced_itm_atm_otm_effects() -> TestResult {
    for (kind, level, gross) in [
        (OptionKind::Call, 105, 150_000),
        (OptionKind::Call, 100, 0),
        (OptionKind::Call, 95, 0),
        (OptionKind::Put, 95, 150_000),
        (OptionKind::Put, 100, 0),
        (OptionKind::Put, 105, 0),
    ] {
        let policy = policy(
            kind,
            SettlementRounding::Down,
            SettlementCharges::new(FeeBasis::EveryContract, 2),
        )?;
        let fixing = fixing(&policy, level, 0)?;
        let position = PositionCost::new(3, 1234)?;
        let effect =
            calculated(policy.calculate(at(12)?, position, 20, std::slice::from_ref(&fixing))?)?;
        assert_eq!(effect.amounts(), (3, gross, 6));
        let (cash, postings) = effect.effects();
        assert_eq!(cash, 20 + gross - 6);
        assert_eq!(
            postings,
            [i128::from(gross) - 6, -1234, 1234 - i128::from(gross), 6]
        );
        assert_eq!(postings.into_iter().sum::<i128>(), 0);
        assert_eq!(effect.provenance(), (policy.identity(), fixing.identity()));
        assert_eq!(position.values(), (3, 1234)); // Calculation does not commit closure.
        assert_eq!(
            effect,
            calculated(policy.calculate(at(30)?, position, 20, &[fixing])?)?
        );
    }
    Ok(())
}
#[test]
fn aggregate_rounding_and_half_even_are_explicit_not_per_contract() -> TestResult {
    for (rounding, units, expected) in [
        (SettlementRounding::Down, 1, 0),
        (SettlementRounding::Up, 1, 1),
        (SettlementRounding::NearestEven, 1, 0),
        (SettlementRounding::NearestEven, 3, 2),
        (SettlementRounding::Down, 3, 1),
    ] {
        let policy = policy(
            OptionKind::Call,
            rounding,
            SettlementCharges::new(FeeBasis::InTheMoney, 0),
        )?;
        // Each contract has exactly half a cent intrinsic payout.
        let effect = calculated(policy.calculate(
            at(12)?,
            PositionCost::new(units, 7)?,
            0,
            &[fixing(&policy, 10_000_005, 5)?],
        )?)?;
        assert_eq!(effect.amounts(), (units, expected, 0));
        assert_eq!(effect.effects().1.into_iter().sum::<i128>(), 0);
    }
    let policy = policy(
        OptionKind::Call,
        SettlementRounding::NearestEven,
        SettlementCharges::new(FeeBasis::InTheMoney, 0),
    )?;
    let effect = calculated(policy.calculate(
        at(12)?,
        PositionCost::new(1, 7)?,
        0,
        &[fixing(&policy, 10_000_015, 5)?],
    )?)?;
    assert_eq!(effect.amounts(), (1, 2, 0)); // 1.5 cents ties to even 2.
    Ok(())
}
#[test]
fn missing_not_due_future_and_ambiguous_evidence_never_complete_expiry() -> TestResult {
    let policy = policy(
        OptionKind::Call,
        SettlementRounding::Down,
        SettlementCharges::new(FeeBasis::EveryContract, 0),
    )?;
    let position = PositionCost::new(1, 200)?;
    let evidence = fixing(&policy, 105, 0)?;
    assert_eq!(
        policy.calculate(at(9)?, position, 0, &[])?,
        SettlementDecision::Deferred(DeferredReason::NotDue)
    );
    assert_eq!(
        policy.calculate(at(10)?, position, 0, &[])?,
        SettlementDecision::Deferred(DeferredReason::MissingEvidence)
    );
    assert_eq!(
        policy.calculate(at(11)?, position, 0, std::slice::from_ref(&evidence))?,
        SettlementDecision::Deferred(DeferredReason::NotKnown)
    );
    assert_eq!(
        policy.calculate(at(12)?, position, 0, &[evidence.clone(), evidence.clone()]),
        Err(SettlementError::Evidence)
    );
    let other = self::policy(
        OptionKind::Put,
        SettlementRounding::Down,
        SettlementCharges::new(FeeBasis::EveryContract, 0),
    )?;
    assert_eq!(
        other.calculate(at(12)?, position, 0, &[evidence]),
        Err(SettlementError::Evidence)
    );
    assert_eq!(position.values(), (1, 200));
    Ok(())
}
#[test]
fn fees_require_explicit_basis_and_available_unreserved_funding() -> TestResult {
    let position = PositionCost::new(2, 500)?;
    for (basis, fees) in [(FeeBasis::EveryContract, 6), (FeeBasis::InTheMoney, 0)] {
        let policy = policy(
            OptionKind::Call,
            SettlementRounding::Down,
            SettlementCharges::new(basis, 3),
        )?;
        let evidence = fixing(&policy, 95, 0)?;
        let effect =
            calculated(policy.calculate(at(12)?, position, 6, std::slice::from_ref(&evidence))?)?;
        assert_eq!(effect.amounts(), (2, 0, fees));
        if fees > 0 {
            assert_eq!(
                policy.calculate(at(12)?, position, 5, &[evidence]),
                Err(SettlementError::Funding)
            );
        }
    }
    // ITM is exact intrinsic, not its rounded cent result; do not silently waive fees.
    let policy = policy(
        OptionKind::Call,
        SettlementRounding::Down,
        SettlementCharges::new(FeeBasis::InTheMoney, 1),
    )?;
    assert_eq!(
        policy.calculate(
            at(12)?,
            PositionCost::new(1, 0)?,
            0,
            &[fixing(&policy, 10_000_005, 5)?]
        ),
        Err(SettlementError::Funding)
    );
    Ok(())
}
#[test]
fn timing_shapes_and_numeric_exhaustion_fail_closed() -> TestResult {
    let semantics = SemanticReference::new(ReferenceId::new(8)?, ReferenceRevision::new(1)?);
    let charges = SettlementCharges::new(FeeBasis::EveryContract, 0);
    for (exercise, settlement, underlying) in [
        (
            ExerciseStyle::American,
            SettlementStyle::Cash,
            Underlying::Index(InstrumentId::new(1)?),
        ),
        (
            ExerciseStyle::European,
            SettlementStyle::Cash,
            Underlying::Equity(InstrumentId::new(1)?),
        ),
        (
            ExerciseStyle::European,
            SettlementStyle::Physical(DeliverableReference::new(
                ReferenceId::new(10)?,
                ReferenceRevision::new(1)?,
            )),
            Underlying::Index(InstrumentId::new(1)?),
        ),
    ] {
        assert_eq!(
            SettlementPolicy::new(
                specification(OptionKind::Call, exercise, settlement, underlying)?,
                semantics,
                SourceId::new(9)?,
                at(8)?,
                at(14)?,
                SettlementRounding::Down,
                charges
            ),
            Err(SettlementError::Unsupported)
        );
    }
    let spec = specification(
        OptionKind::Call,
        ExerciseStyle::European,
        SettlementStyle::Cash,
        Underlying::Index(InstrumentId::new(1)?),
    )?;
    for (fix, known) in [(-1, 14), (11, 14), (8, 7)] {
        assert_eq!(
            SettlementPolicy::new(
                spec.clone(),
                semantics,
                SourceId::new(9)?,
                at(fix)?,
                at(known)?,
                SettlementRounding::Down,
                charges
            ),
            Err(SettlementError::Timing)
        );
    }
    let policy = policy(OptionKind::Call, SettlementRounding::Down, charges)?;
    for known in [7, 15] {
        assert_eq!(
            SyntheticFinalFixing::new(
                &policy,
                ObservationId::new(1)?,
                at(known)?,
                IndexPoints::new(100, 0)?
            ),
            Err(SettlementError::Timing)
        );
    }
    assert_eq!(
        policy.calculate(at(12)?, PositionCost::new(0, 0)?, 0, &[]),
        Err(SettlementError::Position)
    );
    assert_eq!(
        policy.calculate(
            at(12)?,
            PositionCost::new(1, 0)?,
            u64::MAX,
            &[fixing(&policy, 105, 0)?]
        ),
        Err(SettlementError::Arithmetic)
    );
    assert_eq!(
        policy.calculate(
            at(12)?,
            PositionCost::new(u64::MAX, 0)?,
            0,
            &[fixing(&policy, 999_999_999_999_999_999_999_999_999, 0)?]
        ),
        Err(SettlementError::Arithmetic)
    );
    let costly = self::policy(
        OptionKind::Call,
        SettlementRounding::Down,
        SettlementCharges::new(FeeBasis::EveryContract, u64::MAX),
    )?;
    assert_eq!(
        costly.calculate(
            at(12)?,
            PositionCost::new(2, 0)?,
            u64::MAX,
            &[fixing(&costly, 100, 0)?]
        ),
        Err(SettlementError::Arithmetic)
    );
    Ok(())
}
#[test]
fn canonical_identity_binds_configuration_and_evidence_and_diagnostics_redact_values() -> TestResult
{
    let base = policy(
        OptionKind::Call,
        SettlementRounding::Down,
        SettlementCharges::new(FeeBasis::EveryContract, 0),
    )?;
    let changed = policy(
        OptionKind::Call,
        SettlementRounding::Up,
        SettlementCharges::new(FeeBasis::EveryContract, 0),
    )?;
    assert_ne!(base.identity(), changed.identity());
    assert_eq!(base.identity(), ArtifactDigest::of(&base.canonical_bytes()));
    let evidence = fixing(&base, 123_456_789, 4)?;
    assert_eq!(
        evidence.identity(),
        ArtifactDigest::of(&evidence.canonical_bytes())
    );
    assert_ne!(
        evidence.identity(),
        fixing(&base, 123_456_788, 4)?.identity()
    );
    let normalized = fixing(&base, 10000, 2)?;
    assert_eq!(normalized.identity(), fixing(&base, 100, 0)?.identity());
    let effect = calculated(base.calculate(
        at(12)?,
        PositionCost::new(1, 12_345)?,
        0,
        std::slice::from_ref(&evidence),
    )?)?;
    assert_eq!(format!("{base:?}"), "SettlementPolicy(..)");
    assert_eq!(format!("{evidence:?}"), "SyntheticFinalFixing(..)");
    assert_eq!(format!("{effect:?}"), "SettlementCalculation(..)");
    fn auto_traits<T: Send + Sync + Clone + std::fmt::Debug + Eq>() {}
    auto_traits::<SettlementPolicy>();
    auto_traits::<SyntheticFinalFixing>();
    auto_traits::<SettlementCalculation>();
    Ok(())
}

#[test]
fn exact_fractional_payoffs_balance_across_a_bounded_reference_matrix() -> TestResult {
    // Independent reference: a 10^-5-point difference times 100 USD/point
    // is 0.1 cent per contract. No floating point or random seed.
    for kind in [OptionKind::Call, OptionKind::Put] {
        for rounding in [
            SettlementRounding::Down,
            SettlementRounding::Up,
            SettlementRounding::NearestEven,
        ] {
            let policy = policy(
                kind,
                rounding,
                SettlementCharges::new(FeeBasis::EveryContract, 1),
            )?;
            for offset in -20_i128..=20 {
                let evidence = fixing(&policy, 10_000_000 + offset, 5)?;
                let intrinsic = if kind == OptionKind::Call {
                    offset.max(0)
                } else {
                    (-offset).max(0)
                };
                for units in 1_u64..=15 {
                    let tenths = u64::try_from(intrinsic)? * units;
                    let whole = tenths / 10;
                    let remainder = tenths % 10;
                    let increase = match rounding {
                        SettlementRounding::Down => false,
                        SettlementRounding::Up => remainder > 0,
                        SettlementRounding::NearestEven => {
                            remainder > 5 || (remainder == 5 && !whole.is_multiple_of(2))
                        }
                        _ => return Err("unexpected rounding variant".into()),
                    };
                    let expected = whole + u64::from(increase);
                    let cost = units * 7 + 3;
                    let effect = calculated(policy.calculate(
                        at(12)?,
                        PositionCost::new(units, cost)?,
                        10_000,
                        std::slice::from_ref(&evidence),
                    )?)?;
                    assert_eq!(effect.amounts(), (units, expected, units));
                    let (cash, postings) = effect.effects();
                    assert_eq!(cash, 10_000 + expected - units);
                    assert_eq!(postings.get(1), Some(&-i128::from(cost)));
                    assert_eq!(postings.into_iter().sum::<i128>(), 0);
                }
            }
        }
    }
    Ok(())
}

#[test]
fn account_attribution_conserves_cents_independent_of_strategy_arrival_order() -> TestResult {
    use edgeagent_simulation_domain::risk::RiskStrategyId;
    let policy = policy(
        OptionKind::Call,
        SettlementRounding::NearestEven,
        SettlementCharges::new(FeeBasis::EveryContract, 1),
    )?;
    let result = calculated(policy.calculate_account(
        at(12)?,
        PositionCost::new(3, 18000)?,
        100,
        &[fixing(&policy, 10_000_005, 5)?],
    )?)?;
    let first = RiskStrategyId::new(1)?;
    let second = RiskStrategyId::new(2)?;
    let positions = [
        (second, PositionCost::new(1, 6000)?),
        (first, PositionCost::new(2, 12000)?),
    ];
    let allocations = result.allocate(&positions)?;
    let mut reversed = positions;
    reversed.reverse();
    assert_eq!(allocations, result.allocate(&reversed)?);
    let actual: Vec<_> = allocations
        .iter()
        .copied()
        .map(SettlementAttribution::parts)
        .collect();
    assert_eq!(
        actual,
        vec![
            (first, 2, 1, 2, [-1, -12000, 11999, 2]),
            (second, 1, 1, 1, [0, -6000, 5999, 1])
        ]
    );
    let mut totals = [0_i128; 4];
    for allocation in &allocations {
        for (total, n) in totals.iter_mut().zip(allocation.parts().4) {
            *total += n;
        }
    }
    assert_eq!(totals, result.effects().1);
    assert_eq!(
        result.allocate(&[(first, PositionCost::new(3, 17999)?)]),
        Err(SettlementError::Position)
    );
    assert_eq!(
        result.allocate(&[
            (first, PositionCost::new(1, 6000)?),
            (first, PositionCost::new(2, 12000)?)
        ]),
        Err(SettlementError::Position)
    );
    assert_eq!(
        result.allocate(&[(first, PositionCost::new(0, 0)?); 33]),
        Err(SettlementError::Position)
    );
    let empty = PositionCost::new(0, 0)?;
    assert_eq!(
        policy.calculate_account(at(12)?, empty, 99, &[])?,
        SettlementDecision::Deferred(DeferredReason::MissingEvidence)
    );
    let empty_result =
        calculated(policy.calculate_account(at(12)?, empty, 99, &[fixing(&policy, 105, 0)?])?)?;
    assert_eq!(empty_result.amounts(), (0, 0, 0));
    assert_eq!(empty_result.effects(), (99, [0; 4]));
    assert!(empty_result.allocate(&[])?.is_empty());
    Ok(())
}

#[test]
fn fractional_contract_terms_and_net_cash_boundaries_do_not_assume_a_standard_multiplier()
-> TestResult {
    let original = specification(
        OptionKind::Call,
        ExerciseStyle::European,
        SettlementStyle::Cash,
        Underlying::Index(InstrumentId::new(1)?),
    )?;
    let spec = original.value();
    let terms = OptionTerms::new(
        OptionKind::Call,
        OptionPoints::new(100_125, 3)?,
        OptionPoints::new(1, 2)?,
        UsdPerPoint::new(325, 2)?,
        ExerciseStyle::European,
        SettlementStyle::Cash,
    )?;
    let custom = ReferenceVersion::new(
        original.id(),
        original.revision(),
        original.source(),
        original.known_at(),
        *original.checksum(),
        OptionSpecification::new(
            spec.id(),
            spec.underlying(),
            terms,
            spec.schedule(),
            spec.effective(),
        )?,
    );
    let policy = SettlementPolicy::new(
        custom,
        SemanticReference::new(ReferenceId::new(8)?, ReferenceRevision::new(1)?),
        SourceId::new(9)?,
        at(8)?,
        at(14)?,
        SettlementRounding::NearestEven,
        SettlementCharges::new(FeeBasis::EveryContract, 2),
    )?;
    // (100.4 - 100.125) * 3.25 * 7 = 6.25625 USD; whole-position rounding -> 626 cents.
    let effect = calculated(policy.calculate(
        at(12)?,
        PositionCost::new(7, 700)?,
        0,
        &[fixing(&policy, 1004, 1)?],
    )?)?;
    assert_eq!(effect.amounts(), (7, 626, 14));
    assert_eq!(effect.effects(), (612, [612, -700, 74, 14]));
    let offsetting = self::policy(
        OptionKind::Call,
        SettlementRounding::Down,
        SettlementCharges::new(FeeBasis::EveryContract, 50_000),
    )?;
    let unchanged = calculated(offsetting.calculate(
        at(12)?,
        PositionCost::new(1, 1)?,
        u64::MAX,
        &[fixing(&offsetting, 105, 0)?],
    )?)?;
    assert_eq!(unchanged.effects().0, u64::MAX);
    Ok(())
}

#[test]
fn source_semantics_specification_and_receipt_bounds_cannot_reuse_an_old_fixing() -> TestResult {
    let spec = specification(
        OptionKind::Call,
        ExerciseStyle::European,
        SettlementStyle::Cash,
        Underlying::Index(InstrumentId::new(1)?),
    )?;
    let semantic = SemanticReference::new(ReferenceId::new(8)?, ReferenceRevision::new(1)?);
    let charges = SettlementCharges::new(FeeBasis::EveryContract, 0);
    let baseline = policy(OptionKind::Call, SettlementRounding::Down, charges)?;
    let evidence = fixing(&baseline, 105, 0)?;
    let position = PositionCost::new(1, 1)?;
    let changed_spec = ReferenceVersion::new(
        spec.id(),
        ReferenceRevision::new(2)?,
        spec.source(),
        spec.known_at(),
        *spec.checksum(),
        *spec.value(),
    );
    let changed_checksum = ReferenceVersion::new(
        spec.id(),
        spec.revision(),
        spec.source(),
        spec.known_at(),
        [8; 32],
        *spec.value(),
    );
    for (reference, rules, source, fixing_at, known_by, fees) in [
        (
            spec.clone(),
            semantic,
            SourceId::new(10)?,
            at(8)?,
            at(14)?,
            charges,
        ),
        (
            spec.clone(),
            SemanticReference::new(ReferenceId::new(8)?, ReferenceRevision::new(2)?),
            SourceId::new(9)?,
            at(8)?,
            at(14)?,
            charges,
        ),
        (
            changed_spec,
            semantic,
            SourceId::new(9)?,
            at(8)?,
            at(14)?,
            charges,
        ),
        (
            changed_checksum,
            semantic,
            SourceId::new(9)?,
            at(8)?,
            at(14)?,
            charges,
        ),
        (
            spec.clone(),
            semantic,
            SourceId::new(9)?,
            at(9)?,
            at(14)?,
            charges,
        ),
        (
            spec.clone(),
            semantic,
            SourceId::new(9)?,
            at(8)?,
            at(15)?,
            charges,
        ),
        (
            spec.clone(),
            semantic,
            SourceId::new(9)?,
            at(8)?,
            at(14)?,
            SettlementCharges::new(FeeBasis::EveryContract, 1),
        ),
        (
            spec.clone(),
            semantic,
            SourceId::new(9)?,
            at(8)?,
            at(14)?,
            SettlementCharges::new(FeeBasis::InTheMoney, 0),
        ),
    ] {
        let changed = SettlementPolicy::new(
            reference,
            rules,
            source,
            fixing_at,
            known_by,
            SettlementRounding::Down,
            fees,
        )?;
        assert_ne!(baseline.identity(), changed.identity());
        assert_eq!(
            changed.calculate(at(12)?, position, 0, std::slice::from_ref(&evidence)),
            Err(SettlementError::Evidence)
        );
    }
    let boundary = SyntheticFinalFixing::new(
        &baseline,
        ObservationId::new(2)?,
        at(14)?,
        IndexPoints::new(105, 0)?,
    )?;
    assert!(matches!(
        baseline.calculate(at(14)?, position, 0, &[boundary])?,
        SettlementDecision::Calculated(_)
    ));
    let later_reference = ReferenceVersion::new(
        spec.id(),
        spec.revision(),
        spec.source(),
        utc(13)?,
        *spec.checksum(),
        *spec.value(),
    );
    let later_policy = SettlementPolicy::new(
        later_reference,
        semantic,
        SourceId::new(9)?,
        at(8)?,
        at(14)?,
        SettlementRounding::Down,
        charges,
    )?;
    let later_evidence = fixing(&later_policy, 105, 0)?;
    assert_eq!(
        later_policy.calculate(at(12)?, position, 0, std::slice::from_ref(&later_evidence))?,
        SettlementDecision::Deferred(DeferredReason::NotKnown)
    );
    assert!(matches!(
        later_policy.calculate(at(13)?, position, 0, &[later_evidence])?,
        SettlementDecision::Calculated(_)
    ));
    Ok(())
}
