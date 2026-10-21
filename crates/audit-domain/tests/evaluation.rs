//! Invented cash/European option path. No provider data or performance promises.
#[path = "evaluation/support.rs"]
mod support;
use edgeagent_audit_domain::evaluation::*;
use edgeagent_contracts::audit::AccountAuditFrame;
use edgeagent_simulation_domain::{Run, execution::ExecutionProfile};
use std::time::Duration;
use support::*;

fn report(plan: &EvaluationPlan<'_>, owned: &[OwnedTrace]) -> Result<EvaluationReport> {
    let samples: Vec<_> = owned.iter().map(OwnedTrace::samples).collect();
    Ok(plan.evaluate(
        &samples
            .iter()
            .map(|s| EvaluationTrace {
                samples: s,
                owner_revision: 2,
            })
            .collect::<Vec<_>>(),
    )?)
}
#[test]
fn actual_execution_and_settlement_expose_cost_latency_and_liquidity_sensitivity() -> Result {
    let fixture = Fixture::new()?;
    let manifest = fixture.manifest()?;
    let plan = EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &fixture.cases()?)?;
    let runs = (0..4)
        .map(|i| fixture.execute(&manifest, i, false))
        .collect::<Result<Vec<_>>>()?;
    let output = report(&plan, &runs)?;
    assert_eq!(
        output
            .cases()
            .iter()
            .map(|c| (c.net_cents, c.fees_cents, c.filled_units))
            .collect::<Vec<_>>(),
        [
            (131_994, 6, [0, 3]),
            (131_993, 7, [0, 3]),
            (0, 0, [0, 0]),
            (43_998, 2, [0, 1])
        ]
    );
    assert_eq!(output.cases()[1].net_delta_cents, -1);
    assert_eq!(output.cases()[1].fee_delta_cents, 1);
    assert_eq!(output.cases()[0].gross_cash_flow_cents, 168_000);
    assert_eq!(output.cases()[1].book_drawdown_cents, 1);
    assert!(
        output
            .cases()
            .iter()
            .all(|c| c.transitions == 2 && c.orders == 1 && !c.risk_breached)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.encode()?)?;
    assert_eq!(json["uncertainty"], "not_estimated_single_synthetic_path");
    Ok(())
}
#[test]
fn repeated_and_checkpoint_resumed_paths_have_identical_canonical_reports() -> Result {
    let fixture = Fixture::new()?;
    let manifest = fixture.manifest()?;
    let plan = EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &fixture.cases()?)?;
    let straight = (0..4)
        .map(|i| fixture.execute(&manifest, i, false))
        .collect::<Result<Vec<_>>>()?;
    let resumed = (0..4)
        .map(|i| fixture.execute(&manifest, i, true))
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(
        report(&plan, &straight)?.encode()?,
        report(&plan, &resumed)?.encode()?
    );
    assert_eq!(
        report(&plan, &straight)?.encode()?,
        report(&plan, &straight)?.encode()?
    );
    assert_eq!(plan.digest(), digest(plan.canonical_bytes()));
    assert_eq!(
        hex(plan.digest()),
        "b76e14c3e53fce8e5445fcbfe0fcc9d5421c01d890d07ff3c040621b30f3aaad"
    );
    Ok(())
}
#[test]
fn frozen_case_census_rejects_omitted_duplicate_and_multifactor_experiments() -> Result {
    let fixture = Fixture::new()?;
    let manifest = fixture.manifest()?;
    let cases = fixture.cases()?;
    assert!(EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &cases[..3]).is_err());
    let mut changed = cases.clone();
    changed[3] = cases[1];
    assert!(EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &changed).is_err());
    let base = fixture.profiles()?[0];
    let multi = ExecutionProfile::new(
        base.identity().0,
        2500,
        1_000_000,
        0,
        Duration::from_secs(1),
        Duration::ZERO,
    )?;
    changed[3] = EvaluationCase::new(
        SensitivityAxis::Liquidity,
        multi,
        digest(b"other-config"),
        digest(b"other-plan"),
        Some((fixture.settlement.identity(), digest(b"postlude"))),
        2,
    );
    assert!(EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &changed).is_err());
    changed = cases.clone();
    changed[0] = EvaluationCase::new(
        SensitivityAxis::Baseline,
        base,
        digest(b"other-config"),
        digest(b"other-plan"),
        Some((fixture.settlement.identity(), digest(b"postlude"))),
        8193,
    );
    assert!(EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &changed).is_err());
    let plan = EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &cases)?;
    let later = EvaluationPlan::new(&manifest, at(-2)?, (at(0)?, at(12)?), &cases)?;
    assert_ne!(plan.digest(), later.digest());
    let mut changed = cases;
    changed[3] = EvaluationCase::new(
        SensitivityAxis::Liquidity,
        fixture.profiles()?[3],
        digest(b"different"),
        digest(b"different-plan"),
        Some((digest(b"different-final-evidence"), digest(b"postlude"))),
        2,
    );
    assert!(EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &changed).is_err());
    Ok(())
}
#[test]
fn overlapping_holdouts_future_marks_and_invalid_continuations_fail_closed() -> Result {
    let fixture = Fixture::new()?;
    let manifest = fixture.manifest()?;
    let cases = fixture.cases()?;
    assert!(matches!(
        EvaluationPlan::new(&manifest, at(0)?, (at(0)?, at(12)?), &cases),
        Err(EvaluationError::Chronology)
    ));
    assert!(EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(2)?), &cases).is_err());
    let plan = EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &cases)?;
    for mutation in 0..4 {
        let mut runs = (0..4)
            .map(|i| fixture.execute(&manifest, i, false))
            .collect::<Result<Vec<_>>>()?;
        match mutation {
            0 => {
                let mut s = runs[0].frames[1].state().clone();
                s.valuations[0].valued_seconds = 4;
                runs[0].frames[1] =
                    AccountAuditFrame::new(s, runs[0].frames[1].previous().map(str::to_owned))?;
            }
            1 => runs[0].checkpoints[1][0] ^= 1,
            2 => runs[0].checkpoints[2] = Run::new(&manifest).checkpoint(),
            _ => runs[0].checkpoints[0] = runs[0].checkpoints[1].clone(),
        }
        assert!(report(&plan, &runs).is_err());
    }
    Ok(())
}
#[test]
fn partial_foreign_conflicting_and_open_owner_histories_produce_no_report() -> Result {
    let fixture = Fixture::new()?;
    let manifest = fixture.manifest()?;
    let plan = EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &fixture.cases()?)?;
    for mutation in 0..7 {
        let mut runs = (0..4)
            .map(|i| fixture.execute(&manifest, i, false))
            .collect::<Result<Vec<_>>>()?;
        match mutation {
            0 => {
                runs[0].frames.pop();
            }
            1 => {
                runs[0].frames[1] = runs[0].frames[0].clone();
            }
            2 => runs.swap(1, 2),
            3 => {
                let mut s = runs[1].frames[0].state().clone();
                s.cash_cents += 1;
                runs[1].frames[0] = AccountAuditFrame::new(s, None)?;
            }
            4 => {
                let mut s = runs[0].frames[2].state().clone();
                s.cash_cents += 1;
                runs[0].frames[2] =
                    AccountAuditFrame::new(s, runs[0].frames[2].previous().map(str::to_owned))?;
            }
            5 => {
                // Internally balanced but still open: never mark this inventory at cost.
                let mut s = runs[0].frames[2].state().clone();
                s.positions[0].quantity = 1;
                s.positions[0].cost_cents = Some(100);
                s.cash_cents -= 100;
                s.postings[0] -= 100;
                s.postings[1] += 100;
                runs[0].frames[2] =
                    AccountAuditFrame::new(s, runs[0].frames[2].previous().map(str::to_owned))?;
            }
            _ => {
                // Legal bootstrap continuation, but a different ID/RNG baseline.
                let mut run = Run::new(&manifest);
                run.next_id()?;
                run.next_id()?;
                runs[1].checkpoints[0] = run.checkpoint();
            }
        }
        assert!(report(&plan, &runs).is_err());
    }
    let runs = (0..4)
        .map(|i| fixture.execute(&manifest, i, false))
        .collect::<Result<Vec<_>>>()?;
    let samples: Vec<_> = runs.iter().map(OwnedTrace::samples).collect();
    assert_eq!(
        plan.evaluate(
            &samples
                .iter()
                .map(|s| EvaluationTrace {
                    samples: s,
                    owner_revision: 3
                })
                .collect::<Vec<_>>()
        ),
        Err(EvaluationError::Incomplete)
    );
    Ok(())
}
#[test]
fn diagnostics_redact_financial_payloads_and_reports_preserve_declared_order() -> Result {
    let fixture = Fixture::new()?;
    let manifest = fixture.manifest()?;
    let plan = EvaluationPlan::new(&manifest, at(-1)?, (at(0)?, at(12)?), &fixture.cases()?)?;
    let runs = (0..4)
        .map(|i| fixture.execute(&manifest, i, false))
        .collect::<Result<Vec<_>>>()?;
    let output = report(&plan, &runs)?;
    assert_eq!(
        output.cases().iter().map(|c| c.axis).collect::<Vec<_>>(),
        [
            SensitivityAxis::Baseline,
            SensitivityAxis::Cost,
            SensitivityAxis::Latency,
            SensitivityAxis::Liquidity
        ]
    );
    let diagnostic = format!("{plan:?} {output:?} {:?}", runs[0].samples()[0]);
    assert!(!diagnostic.contains(&"1".repeat(32)) && !diagnostic.contains("100000"));
    assert_eq!(
        format!("{}", EvaluationError::Scope),
        "frozen evaluation rejected: Scope"
    );
    Ok(())
}
