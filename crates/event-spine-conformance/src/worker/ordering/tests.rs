use super::{Decision, decide};

#[test]
fn accepted_content_takes_precedence_over_later_revisions() {
    for current in [1, 2, 1_000_001] {
        assert_eq!(decide(current, 0, 42, Some(42)), Decision::Duplicate);
        assert_eq!(decide(current, 0, 99, Some(42)), Decision::Conflict);
    }
}

#[test]
fn only_the_exact_predecessor_permits_a_new_transition() {
    for (current, expected, decision) in [
        (0, 0, Decision::Apply),
        (5, 5, Decision::Apply),
        (1_000_000, 1_000_000, Decision::Apply),
        (0, 1, Decision::MissingPredecessor),
        (1, 0, Decision::Stale),
    ] {
        assert_eq!(decide(current, expected, 42, None), decision);
    }
}
