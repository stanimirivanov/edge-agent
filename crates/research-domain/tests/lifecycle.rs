use edgeagent_research_domain::lifecycle::{Head, State};
#[test]
fn explicit_lifecycle_time_order_and_final_states_are_irreversible() {
    let base = Head {
        state: State::Published,
        revision: 0,
        effective_at_ms: 100,
    };
    for state in [
        State::Invalidated,
        State::Expired,
        State::Withdrawn,
        State::Corrected,
    ] {
        let time = if state == State::Expired { 200 } else { 100 };
        let next = base.transition(1, time, 200, state);
        assert!(next.is_ok());
        assert!(base.transition(2, time, 200, state).is_err());
        assert!(base.transition(1, 99, 200, state).is_err());
        if let Ok(head) = next {
            assert!(head.transition(1, time, 200, state).is_err());
            for target in [
                State::Published,
                State::Invalidated,
                State::Expired,
                State::Withdrawn,
                State::Corrected,
            ] {
                assert_eq!(
                    head.transition(2, time, 200, target).is_ok(),
                    matches!(state, State::Invalidated | State::Expired)
                        && matches!(target, State::Withdrawn | State::Corrected)
                );
            }
        }
    }
    assert!(base.transition(1, 199, 200, State::Expired).is_err());
    assert!(
        Head {
            revision: u64::MAX,
            ..base
        }
        .transition(1, 200, 200, State::Expired)
        .is_err()
    );
}
