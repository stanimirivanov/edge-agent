use edgeagent_market_data_domain::InstrumentId;
use edgeagent_research_domain::policy::*;
type Result = std::result::Result<(), Box<dyn std::error::Error>>;
fn ids(values: &[u128]) -> std::result::Result<Vec<InstrumentId>, Box<dyn std::error::Error>> {
    Ok(values
        .iter()
        .map(|v| InstrumentId::new(*v))
        .collect::<std::result::Result<_, _>>()?)
}
fn intent(direction: Direction, horizon_sessions: u16) -> Intent {
    Intent {
        direction,
        horizon_sessions,
    }
}
#[test]
fn boundaries_narrowing_and_order_independent_reason_precedence() -> Result {
    let policy = Policy::new([1; 32], [2; 32], ids(&[1, 3])?, 5)?;
    let approved = policy.assess(&ids(&[3, 1])?, intent(Direction::Long, 5))?;
    assert_eq!(approved.status(), Status::Approved);
    assert_eq!(approved.eligible(), ids(&[1, 3])?);
    assert!(approved.reasons().is_empty());
    assert_eq!(approved.revision(), [1; 32]);
    let narrowed = policy.assess(&ids(&[3, 2, 1])?, intent(Direction::Long, 1))?;
    assert_eq!(narrowed.status(), Status::Narrowed);
    assert_eq!(narrowed.eligible(), ids(&[1, 3])?);
    assert_eq!(narrowed.reasons(), [Reason::InstrumentNotAllowed]);
    let denied = policy.assess(&ids(&[2])?, intent(Direction::Long, 1))?;
    assert_eq!(denied.status(), Status::Abstained);
    assert!(denied.eligible().is_empty());
    for candidates in [ids(&[1, 2, 3])?, ids(&[3, 2, 1])?, ids(&[2, 1, 3])?] {
        let denied = policy.assess(&candidates, intent(Direction::Short, 6))?;
        assert_eq!(denied.status(), Status::Abstained);
        assert!(denied.eligible().is_empty());
        assert_eq!(
            denied
                .reasons()
                .iter()
                .map(|r| r.code())
                .collect::<Vec<_>>(),
            vec![
                "instrument_not_allowed",
                "direction_not_allowed",
                "horizon_exceeded"
            ]
        );
    }
    for i in [intent(Direction::Short, 5), intent(Direction::Long, 6)] {
        assert_eq!(policy.assess(&ids(&[1])?, i)?.status(), Status::Abstained);
    }
    Ok(())
}
#[test]
fn malformed_policies_and_intents_never_become_valid_denials() -> Result {
    for allowed in [
        vec![],
        ids(&[1, 1])?,
        ids(&[2, 1])?,
        ids(&(1..=65).collect::<Vec<_>>())?,
    ] {
        assert!(Policy::new([1; 32], [2; 32], allowed, 1).is_err());
    }
    assert!(Policy::new([0; 32], [2; 32], ids(&[1])?, 1).is_err());
    assert!(Policy::new([1; 32], [0; 32], ids(&[1])?, 1).is_err());
    for horizon in [0, 253] {
        assert!(Policy::new([1; 32], [2; 32], ids(&[1])?, horizon).is_err());
    }
    let policy = Policy::new([1; 32], [2; 32], ids(&(1..=64).collect::<Vec<_>>())?, 252)?;
    assert_eq!(
        policy
            .assess(
                &ids(&(1..=64).collect::<Vec<_>>())?,
                intent(Direction::Long, 252)
            )?
            .status(),
        Status::Approved
    );
    for candidates in [vec![], ids(&[1, 1])?, ids(&(1..=65).collect::<Vec<_>>())?] {
        assert!(
            policy
                .assess(&candidates, intent(Direction::Long, 1))
                .is_err()
        );
    }
    for horizon in [0, 253] {
        assert!(
            policy
                .assess(&ids(&[1])?, intent(Direction::Long, horizon))
                .is_err()
        );
    }
    assert_eq!(
        InvalidInput.to_string(),
        "invalid research eligibility input"
    );
    Ok(())
}
