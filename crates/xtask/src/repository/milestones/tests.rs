use super::*;

#[test]
fn parses_edgeagent_milestone_titles() {
    assert_eq!(
        parse_title("M01 - Rust engineering foundation", 4),
        Some(Milestone {
            number: 1,
            title: "M01 - Rust engineering foundation".to_owned(),
            outcome: String::new(),
            line: 4,
        })
    );
    assert!(parse_title("M1 - Missing zero", 1).is_none());
    assert!(parse_title("M00 - Reserved", 1).is_none());
    assert!(parse_title("M01-No spaces", 1).is_none());
    assert!(parse_title("M01 -  Extra space", 1).is_none());
    assert!(parse_title("M01 - \u{200b}", 1).is_none());
}

#[test]
fn reads_index_and_direct_level_two_boundaries() {
    let body = "# Roadmap\n\n## TL;DR\n\nSummary.\n\n## Milestone index\n\n\
                | Milestone | Outcome |\n| --- | --- |\n\
                | M01 - First outcome | First becomes true. |\n\
                | M02 - Second outcome | Second becomes true. |\n\n\
                ## M01 - First outcome\n\n**Outcome:** First becomes true.\n\nWork items.\n\n\
                ## M02 - Second outcome\n\n**Outcome:** Second becomes true.\n\nWork items.\n\n\
                ## Planning rules\n\nRules.\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (index, index_violations) = read_index(&document);
    let (boundaries, boundary_violations) = read_boundaries(&document);

    assert!(
        index_violations.is_empty(),
        "violations: {index_violations:#?}; rows: {:#?}",
        document.table_rows()
    );
    assert!(boundary_violations.is_empty());
    assert_eq!(index.len(), 2);
    assert_eq!(boundaries.len(), 2);
    assert!(validate_index_and_boundaries(&index, &boundaries).is_empty());
}

#[test]
fn rejects_a_milestone_index_split_across_tables() {
    let body = "# Roadmap\n\n## Milestone index\n\n\
                | Milestone | Outcome |\n| --- | --- |\n\
                | M01 - First outcome | First becomes true. |\n\n\
                | M02 - Second outcome | Second becomes true. |\n| --- | --- |\n\
                | M03 - Third outcome | Third becomes true. |\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (_, violations) = read_index(&document);

    assert!(violations.iter().any(|violation| {
        violation
            .message
            .contains("must contain exactly one Markdown table")
    }));
    assert!(violations.iter().any(|violation| {
        violation
            .message
            .contains("additional rendered table header")
    }));
}

#[test]
fn requires_the_canonical_index_header_as_the_first_row() {
    let body = "# Roadmap\n\n## Milestone index\n\n\
                | M01 - First outcome | First becomes true. |\n| --- | --- |\n\
                | M02 - Second outcome | Second becomes true. |\n\
                | Milestone | Outcome |\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (index, violations) = read_index(&document);

    assert_eq!(index.len(), 1);
    assert!(violations.iter().any(|violation| {
        violation
            .message
            .contains("must begin with the canonical `Milestone | Outcome` header row")
    }));
    assert!(violations.iter().any(|violation| {
        violation
            .message
            .contains("header may appear only as the first table row")
    }));
}

#[test]
fn reports_missing_or_divergent_boundary_outcomes() {
    let body = "# Roadmap\n\n## Milestone index\n\n\
                | Milestone | Outcome |\n| --- | --- |\n\
                | M01 - First | Indexed outcome. |\n\
                | M02 - Second | Second outcome. |\n\n\
                ## M01 - First\n\n**Outcome:** Different outcome.\n\n\
                ## M02 - Second\n\nWork items first.\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (index, _) = read_index(&document);
    let (boundaries, mut violations) = read_boundaries(&document);
    violations.extend(validate_index_and_boundaries(&index, &boundaries));

    assert!(
        violations
            .iter()
            .any(|violation| { violation.message.contains("differs from index outcome") })
    );
    assert!(violations.iter().any(|violation| {
        violation
            .message
            .contains("has no visible `Outcome:` statement")
    }));
}

#[test]
fn rejects_raw_html_and_indirect_milestone_boundaries() {
    let body = "# Roadmap\n\n## Milestone index\n\n\
                | Milestone | Outcome |\n| --- | --- |\n\
                | M01 - First | First outcome. |\n\n\
                ### M01 - First\n\n<span hidden>Outcome: First outcome.</span>\n";
    let document = crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body);

    let (boundaries, violations) = read_boundaries(&document);

    assert!(boundaries.is_empty());
    assert!(
        violations
            .iter()
            .any(|violation| { violation.message.contains("no direct level-two milestone") })
    );
}

#[test]
fn raw_html_is_rejected_only_in_policy_bearing_boundary_fields() {
    let outcome_html = "# Roadmap\n\n## M01 - First\n\n**Outcome:** <span>First outcome.</span>\n";
    let document =
        crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", outcome_html);
    let (_, violations) = read_boundaries(&document);
    assert!(
        violations
            .iter()
            .any(|violation| { violation.message.contains("outcome uses raw HTML") })
    );

    let body_html = "# Roadmap\n\n## M01 - First\n\n**Outcome:** First outcome.\n\n<div>Optional detail.</div>\n";
    let document =
        crate::repository::markdown::parse_fragment("docs/roadmap/milestones.md", body_html);
    let (_, violations) = read_boundaries(&document);
    assert!(violations.is_empty());
}

#[test]
fn retained_high_water_rejects_deleting_the_published_tail() {
    let milestones: Vec<_> = (1..LATEST_PUBLISHED_MILESTONE)
        .map(|number| Milestone {
            number,
            title: format!("M{number:02} - Outcome"),
            outcome: "Outcome.".to_owned(),
            line: number,
        })
        .collect();

    let violations = validate_sequence(&milestones);

    assert!(
        violations
            .iter()
            .any(|violation| { violation.message.contains("repository policy records M11") })
    );
}
