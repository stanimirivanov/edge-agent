use std::{
    collections::BTreeMap,
    error::Error,
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn parses_only_canonical_decision_names() {
    assert_eq!(record::parse_file_name("0008-enforce-policy.md"), Some(8));
    assert_eq!(record::parse_file_name("0000-template.md"), None);
    assert_eq!(record::parse_file_name("8-enforce-policy.md"), None);
    assert_eq!(record::parse_file_name("0008-Enforce-policy.md"), None);
    assert_eq!(record::parse_file_name("0008-enforce--policy.md"), None);
}

#[test]
fn published_identity_ledger_is_contiguous_and_canonical() {
    for (index, identity) in PUBLISHED_ADRS.iter().enumerate() {
        let expected_number = index + 1;
        let file_name = identity.path.trim_start_matches(ADR_DIRECTORY);

        assert_eq!(identity.number, expected_number);
        assert_eq!(record::parse_file_name(file_name), Some(expected_number));
        assert!(identity.title.chars().any(char::is_alphanumeric));
    }
}

#[test]
fn validates_edgeagent_lifecycle_values() {
    assert!(lifecycle::valid_status("Accepted"));
    assert!(!lifecycle::valid_status("accepted"));
    assert!(lifecycle::valid_iso_date("2028-02-29"));
    assert!(!lifecycle::valid_iso_date("2026-02-29"));
    assert!(lifecycle::valid_milestone(
        "M01 - Rust engineering foundation"
    ));
    assert!(!lifecycle::valid_milestone("M1 - Foundation"));
    assert!(!lifecycle::valid_milestone("M00 - Foundation"));
    assert_eq!(
        lifecycle::milestone_number("M01 - Rust engineering foundation"),
        Some(1)
    );
}

#[test]
fn accepts_a_canonical_decision_record() {
    let body = canonical_record(8, "Accepted", "", "");
    let document = crate::repository::markdown::parse_fragment("decision.md", &body);
    let mut record = Record::new(8, "docs/decisions/0008-decision-8.md".to_owned());

    let violations = record::validate_record(&document, &mut record);

    assert!(violations.is_empty(), "{violations:?}");
    assert_eq!(record.status.as_deref(), Some("Accepted"));
    assert!(record.supersedes.is_empty());
    assert_eq!(record.superseded_by, None);
}

#[test]
fn rejects_hidden_missing_and_reordered_metadata() {
    let body = canonical_record(8, "Accepted", "", "")
        .replace("- Deciders: EdgeAgent maintainers\n", "")
        .replace(
            "- Date: 2026-09-29\n- Milestone:",
            "- Milestone: M01 - Rust engineering foundation\n- Date: 2026-09-29\n- Ignored:",
        );
    let document = crate::repository::markdown::parse_fragment("decision.md", &body);
    let mut record = Record::new(8, "docs/decisions/0008-decision-8.md".to_owned());

    let violations = record::validate_record(&document, &mut record);
    let rendered = render(&violations);

    assert!(rendered.contains("adr.metadata"));
    assert!(rendered.contains("canonical order"));
}

#[test]
fn rejects_section_addition_or_reordering() {
    let body = canonical_record(8, "Accepted", "", "")
        .replace("## Decision\n", "## Extra\n\nExtra.\n\n## Decision\n")
        .replace("## Alternatives considered\n", "## Consequences moved\n");
    let document = crate::repository::markdown::parse_fragment("decision.md", &body);

    let violations = record::validate_exact_sections(&document, "decision.md");

    assert_eq!(violations.len(), 1);
    assert!(violations[0].to_string().contains("canonical order"));
}

#[test]
fn parses_canonical_supersession_references() -> Result<(), String> {
    assert_eq!(
        supersession::parse_references("ADR-0001, ADR-0003", true)?,
        vec![1, 3]
    );
    assert_eq!(
        supersession::parse_references("", false)?,
        Vec::<usize>::new()
    );
    assert!(supersession::parse_references("ADR 0001", true).is_err());
    assert!(supersession::parse_references("ADR-0001, ADR-0002", false).is_err());
    assert!(supersession::parse_references("ADR-0001, ADR-0001", true).is_err());
    Ok(())
}

#[test]
fn requires_reciprocal_directional_supersession() {
    let mut earlier = Record::new(1, "docs/decisions/0001-first.md".to_owned());
    earlier.status = Some("Superseded".to_owned());
    earlier.superseded_by = Some(2);
    let mut replacement = Record::new(2, "docs/decisions/0002-second.md".to_owned());
    replacement.status = Some("Accepted".to_owned());
    replacement.supersedes = vec![1];
    let valid = BTreeMap::from([(1, earlier.clone()), (2, replacement.clone())]);

    assert!(supersession::validate(&valid).is_empty());

    replacement.supersedes.clear();
    let invalid = BTreeMap::from([(1, earlier), (2, replacement)]);
    let rendered = render(&supersession::validate(&invalid));
    assert!(rendered.contains("reciprocal `Supersedes`"));
}

#[test]
fn superseded_replacement_retains_its_supersession_history() {
    let mut first = Record::new(1, "docs/decisions/0001-first.md".to_owned());
    first.status = Some("Superseded".to_owned());
    first.superseded_by = Some(2);
    let mut second = Record::new(2, "docs/decisions/0002-second.md".to_owned());
    second.status = Some("Superseded".to_owned());
    second.supersedes = vec![1];
    second.superseded_by = Some(3);
    let mut third = Record::new(3, "docs/decisions/0003-third.md".to_owned());
    third.status = Some("Accepted".to_owned());
    third.supersedes = vec![2];
    let records = BTreeMap::from([(1, first), (2, second), (3, third)]);

    assert!(supersession::validate(&records).is_empty());
}

#[test]
fn unaccepted_replacement_cannot_claim_supersession() {
    for status in ["Proposed", "Rejected", "Deprecated"] {
        let mut earlier = Record::new(1, "docs/decisions/0001-first.md".to_owned());
        earlier.status = Some("Superseded".to_owned());
        earlier.superseded_by = Some(2);
        let mut replacement = Record::new(2, "docs/decisions/0002-second.md".to_owned());
        replacement.status = Some(status.to_owned());
        replacement.supersedes = vec![1];
        let records = BTreeMap::from([(1, earlier), (2, replacement)]);

        let rendered = render(&supersession::validate(&records));

        assert!(
            rendered.contains("only an Accepted or already-Superseded decision"),
            "status {status} unexpectedly passed: {rendered}"
        );
    }
}

#[test]
fn retained_high_water_detects_deleting_the_latest_decision() {
    // This literal is an independent published floor, not derived from the
    // identity ledger that the test protects.
    let complete = records_through(10);
    assert!(validate_sequence(&complete).is_empty());

    let truncated = records_through(9);
    let rendered = render(&validate_sequence(&truncated));
    assert!(rendered.contains("repository policy records 0010"));
}

#[test]
fn complete_repository_fixture_satisfies_decision_policy() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::canonical()?;
    let repository = fixture.load()?;

    let violations = check(&repository);

    assert!(violations.is_empty(), "{violations:?}");
    Ok(())
}

#[test]
fn index_status_drift_is_rejected() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::canonical()?;
    let index_path = fixture.root.join(ADR_INDEX);
    let body = fs::read_to_string(&index_path)?;
    fs::write(index_path, body.replacen("| Accepted |", "| Proposed |", 1))?;
    let repository = fixture.load()?;

    let rendered = render(&check(&repository));

    assert!(rendered.contains("index status `Proposed` differs"));
    Ok(())
}

#[test]
fn swapping_published_adr_titles_is_rejected() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::canonical()?;
    let first = published_identity(1)?;
    let second = published_identity(2)?;
    let first_path = fixture.root.join(first.path);
    let second_path = fixture.root.join(second.path);
    let first_body = fs::read_to_string(&first_path)?;
    let second_body = fs::read_to_string(&second_path)?;
    fs::write(
        first_path,
        first_body.replacen(first.title, second.title, 1),
    )?;
    fs::write(
        second_path,
        second_body.replacen(second.title, first.title, 1),
    )?;
    let repository = fixture.load()?;

    let rendered = render(&check(&repository));

    assert!(rendered.contains("[adr.identity]"));
    assert!(rendered.contains("ADR-0001 title differs from canonical history"));
    assert!(rendered.contains("ADR-0002 title differs from canonical history"));
    Ok(())
}

#[test]
fn renaming_a_published_adr_is_rejected_even_when_the_index_follows() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::canonical()?;
    let identity = published_identity(1)?;
    let old_file_name = identity.path.trim_start_matches(ADR_DIRECTORY);
    let new_file_name = "0001-reassigned-workspace-decision.md";
    fs::rename(
        fixture.root.join(identity.path),
        fixture.root.join(ADR_DIRECTORY).join(new_file_name),
    )?;
    let index_path = fixture.root.join(ADR_INDEX);
    let index = fs::read_to_string(&index_path)?;
    fs::write(index_path, index.replacen(old_file_name, new_file_name, 1))?;
    let repository = fixture.load()?;

    let rendered = render(&check(&repository));

    assert!(rendered.contains("[adr.identity]"));
    assert!(rendered.contains("ADR-0001 path differs from canonical history"));
    Ok(())
}

#[test]
fn fenced_index_header_cannot_mask_visible_header_drift() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::canonical()?;
    let index_path = fixture.root.join(ADR_INDEX);
    let body = fs::read_to_string(&index_path)?;
    fs::write(
        index_path,
        body.replacen(
            "| ADR | Status | Decision |",
            "```text\n| ADR | Status | Decision |\n```\n\n| ADR | Lifecycle | Notes |",
            1,
        ),
    )?;
    let repository = fixture.load()?;

    let rendered = render(&check(&repository));

    assert!(rendered.contains("header differs from the canonical"));
    Ok(())
}

#[test]
fn decision_index_cannot_be_split_across_rendered_tables() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::canonical()?;
    let index_path = fixture.root.join(ADR_INDEX);
    let body = fs::read_to_string(&index_path)?;
    let fifth = body
        .lines()
        .find(|line| line.contains("[ADR-0005]"))
        .ok_or_else(|| io::Error::other("canonical fixture has no ADR-0005 index row"))?;
    let split = body.replacen(fifth, &format!("\n{fifth}\n|---|---|---|"), 1);
    fs::write(index_path, split)?;
    let repository = fixture.load()?;

    let rendered = render(&check(&repository));

    assert!(rendered.contains("decision index must contain exactly one Markdown table"));
    assert!(rendered.contains("additional rendered table header"));
    Ok(())
}

#[test]
fn unknown_adr_milestone_is_rejected() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::canonical()?;
    let record_path = fixture.root.join(published_identity(1)?.path);
    let body = fs::read_to_string(&record_path)?;
    fs::write(
        record_path,
        body.replacen(
            "M01 - Rust engineering foundation",
            "M99 - Unknown outcome",
            1,
        ),
    )?;
    let repository = fixture.load()?;

    let rendered = render(&check(&repository));

    assert!(rendered.contains("M99 - Unknown outcome` is not published"));
    Ok(())
}

#[test]
fn adr_milestone_title_drift_is_rejected() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::canonical()?;
    let record_path = fixture.root.join(published_identity(1)?.path);
    let body = fs::read_to_string(&record_path)?;
    fs::write(
        record_path,
        body.replacen(
            "M01 - Rust engineering foundation",
            "M01 - Similar but unpublished title",
            1,
        ),
    )?;
    let repository = fixture.load()?;

    let rendered = render(&check(&repository));

    assert!(rendered.contains("differs from published title `M01 - Rust engineering foundation`"));
    Ok(())
}

#[test]
fn template_section_drift_is_rejected() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::canonical()?;
    let index_path = fixture.root.join(ADR_INDEX);
    let body = fs::read_to_string(&index_path)?;
    fs::write(
        index_path,
        body.replacen("## Compatibility and migration", "## Migration", 1),
    )?;
    let repository = fixture.load()?;

    let rendered = render(&check(&repository));

    assert!(rendered.contains("adr.template"));
    assert!(rendered.contains("sections have drifted"));
    Ok(())
}

#[test]
fn nested_decision_markdown_cannot_bypass_policy() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::canonical()?;
    let archive = fixture.root.join(ADR_DIRECTORY).join("archive");
    fs::create_dir_all(&archive)?;
    fs::write(
        archive.join("legacy.md"),
        "# Unchecked historical decision\n",
    )?;
    let repository = fixture.load()?;

    let rendered = render(&check(&repository));

    assert!(rendered.contains("decision Markdown is nested"));
    assert!(rendered.contains("docs/decisions/archive/legacy.md"));
    Ok(())
}

fn records_through(latest: usize) -> BTreeMap<usize, Record> {
    (1..=latest)
        .map(|number| {
            let mut record = Record::new(
                number,
                format!("docs/decisions/{number:04}-decision-{number}.md"),
            );
            record.status = Some("Accepted".to_owned());
            (number, record)
        })
        .collect()
}

fn render(violations: &[Violation]) -> String {
    violations
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn canonical_record(number: usize, status: &str, supersedes: &str, superseded_by: &str) -> String {
    canonical_record_with_title(
        number,
        &format!("Decision {number}"),
        status,
        supersedes,
        superseded_by,
    )
}

fn canonical_record_with_title(
    number: usize,
    title: &str,
    status: &str,
    supersedes: &str,
    superseded_by: &str,
) -> String {
    format!(
        "# ADR-{number:04}: {title}\n\n\
         - Status: {status}\n\
         - Date: 2026-09-29\n\
         - Milestone: M01 - Rust engineering foundation\n\
         - Deciders: EdgeAgent maintainers\n\
         - Supersedes: {supersedes}\n\
         - Superseded by: {superseded_by}\n\n\
         ## TL;DR\n\nSummary.\n\n\
         ## Context\n\nContext.\n\n\
         ## Decision\n\nDecision.\n\n\
         ## Alternatives considered\n\nAlternatives.\n\n\
         ## Consequences\n\nConsequences.\n\n\
         ## Compatibility and migration\n\nCompatibility.\n\n\
         ## Security and operations\n\nSecurity.\n\n\
         ## Validation\n\nValidation.\n"
    )
}

fn canonical_index() -> String {
    let rows = PUBLISHED_ADRS
        .iter()
        .map(|identity| {
            let file_name = identity.path.trim_start_matches(ADR_DIRECTORY);
            format!(
                "| [ADR-{number:04}]({file_name}) | Accepted | {title}. |",
                number = identity.number,
                title = identity.title,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "# Architecture decision records\n\n\
         ## TL;DR\n\nUse durable decisions.\n\n\
         ## Template\n\n```markdown\n\
         # ADR-NNNN: Decision title\n\n\
         - Status: Proposed\n\
         - Date: YYYY-MM-DD\n\
         - Milestone: MNN - Outcome\n\
         - Deciders:\n\
         - Supersedes:\n\
         - Superseded by:\n\n\
         ## TL;DR\n\nSummarize the decision.\n\n\
         ## Context\n\nContext.\n\n\
         ## Decision\n\nDecision.\n\n\
         ## Alternatives considered\n\nAlternatives.\n\n\
         ## Consequences\n\nConsequences.\n\n\
         ## Compatibility and migration\n\nCompatibility.\n\n\
         ## Security and operations\n\nSecurity.\n\n\
         ## Validation\n\nValidation.\n\
         ```\n\n\
         ## Index\n\n\
         | ADR | Status | Decision |\n\
         |---|---|---|\n\
         {rows}\n"
    )
}

fn published_identity(number: usize) -> io::Result<&'static PublishedIdentity> {
    PUBLISHED_ADRS
        .iter()
        .find(|identity| identity.number == number)
        .ok_or_else(|| io::Error::other(format!("missing published ADR identity {number:04}")))
}

fn canonical_roadmap() -> &'static str {
    "# EdgeAgent implementation milestones\n\n\
     ## Milestone index\n\n\
     | Milestone | Outcome |\n\
     | --- | --- |\n\
     | M01 - Rust engineering foundation | Build and govern the workspace. |\n\n\
     ## M01 - Rust engineering foundation\n\n\
     **Outcome:** Build and govern the workspace.\n"
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn canonical() -> io::Result<Self> {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("edgeagent-adr-policy-{}-{id}", std::process::id()));
        let decisions = root.join(ADR_DIRECTORY);
        fs::create_dir_all(&decisions)?;
        fs::write(root.join(ADR_INDEX), canonical_index())?;
        let roadmap = root.join("docs/roadmap");
        fs::create_dir_all(&roadmap)?;
        fs::write(roadmap.join("milestones.md"), canonical_roadmap())?;
        for identity in PUBLISHED_ADRS {
            fs::write(
                root.join(identity.path),
                canonical_record_with_title(identity.number, identity.title, "Accepted", "", ""),
            )?;
        }
        Ok(Self { root })
    }

    fn load(&self) -> Result<Repository, io::Error> {
        crate::repository::markdown::load(&self.root).map_err(io::Error::other)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
