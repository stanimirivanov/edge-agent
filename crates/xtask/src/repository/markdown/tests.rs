use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn parses_duplicate_anchors_and_ignores_fenced_code_words() -> Result<(), Box<dyn Error>> {
    let document = parse(
        "guide.md".to_owned(),
        "# Guide\n\n## Same heading!\nreal words\n\n## Same heading!\n```text\nignored words here\n```\n"
            .to_owned(),
    );

    assert_eq!(document.word_count, 7);
    assert_eq!(document.headings[1].anchor, "same-heading");
    assert_eq!(document.headings[2].anchor, "same-heading-1");
    Ok(())
}

#[test]
fn reserves_explicitly_colliding_generated_anchors() -> Result<(), Box<dyn Error>> {
    let document = parse(
        "guide.md".to_owned(),
        "# Guide\n\n## Foo\n\n## Foo-1\n\n## Foo\n".to_owned(),
    );

    assert_eq!(document.headings[1].anchor, "foo");
    assert_eq!(document.headings[2].anchor, "foo-1");
    assert_eq!(document.headings[3].anchor, "foo-2");
    Ok(())
}

#[test]
fn github_anchors_remove_non_space_whitespace() {
    let document = parse(
        "guide.md".to_owned(),
        "# Guide\n\n## Market\tData\n\n## Market Data\n".to_owned(),
    );

    assert_eq!(document.headings[1].anchor, "marketdata");
    assert_eq!(document.headings[2].anchor, "market-data");
}

#[test]
fn preserves_table_identity_and_rendered_header_role() {
    let document = parse(
        "guide.md".to_owned(),
        "# Guide\n\n| First | Value |\n| --- | --- |\n| A | B |\n\n\
         | Second | Value |\n| --- | --- |\n| C | D |\n"
            .to_owned(),
    );

    assert_eq!(document.table_rows.len(), 4);
    assert!(document.table_rows[0].is_header);
    assert!(!document.table_rows[1].is_header);
    assert!(document.table_rows[2].is_header);
    assert!(!document.table_rows[3].is_header);
    assert_eq!(document.table_rows[0].table, document.table_rows[1].table);
    assert_ne!(document.table_rows[1].table, document.table_rows[2].table);
    assert_eq!(document.table_rows[2].table, document.table_rows[3].table);
}

#[test]
fn separates_rendered_policy_text_from_image_alt_and_hidden_html() -> Result<(), Box<dyn Error>> {
    let document = parse(
        "guide.md".to_owned(),
        "# ![Visible heading](image.png)\n\n\
         | Milestone | Message |\n|:--|:--|\n\
         | M01 - Visible | <span>Rendered text</span> |\n\
         | M02 - Hidden | <span hidden>Hidden text</span> |\n\
         | M03 - Image | ![Image-only text](image.png) |\n"
            .to_owned(),
    );

    assert_eq!(document.headings[0].text, "");
    assert_eq!(document.headings[0].anchor, "visible-heading");
    assert!(document.visible_text_contains("Rendered text"));
    assert!(document.visible_text_contains("Hidden text"));
    assert!(!document.visible_text_contains("Image-only text"));
    assert!(document.contains_raw_html());
    assert_eq!(document.table_rows[3].cells[1], "");
    Ok(())
}

#[test]
fn validates_inline_reference_and_percent_encoded_anchors() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write(
        "README.md",
        "# Home\n\n[inline](docs/guide.md#repeated-heading-1)\n[reference][guide]\n\n[guide]: docs/guide.md#Repeated%20Heading\n",
    )?;
    fixture.write(
        "docs/guide.md",
        "# Guide\n\n## Repeated heading\n\n## Repeated heading\n",
    )?;
    let repository = load_fixture(fixture.path())?;

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1, "anchors are case-sensitive");
    Ok(())
}

#[test]
fn ignores_markdown_like_links_inside_code_and_raw_html() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write(
        "README.md",
        "# Home\n\n```markdown\n[missing](absent.md)\n```\n<a href=\"absent.md\">raw</a>\n",
    )?;
    let repository = load_fixture(fixture.path())?;

    assert!(check_links(&repository).is_empty());
    Ok(())
}

#[test]
fn validates_unused_reference_definitions() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write("README.md", "# Home\n\n[unused]: missing.md\n")?;
    let repository = load_fixture(fixture.path())?;

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule, "markdown.local-link");
    Ok(())
}

#[test]
fn validates_fragments_for_markdown_targets_outside_the_inventory() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write(
        "README.md",
        "# Home\n\n[valid](draft/guide.md#available)\n[broken](draft/guide.md#missing)\n",
    )?;
    let repository = load_fixture(fixture.path())?;
    fixture.write("draft/guide.md", "# Guide\n\n## Available\n")?;

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("#missing"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn validates_fragments_through_markdown_file_symlinks() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write(
        "README.md",
        "# Home\n\n[broken](.link-target/guide-link.md#missing)\n",
    )?;
    fixture.write("docs/guide.md", "# Guide\n\n## Available\n")?;
    fs::create_dir_all(fixture.path().join(".link-target"))?;
    let repository = load_fixture(fixture.path())?;
    std::os::unix::fs::symlink(
        "../docs/guide.md",
        fixture.path().join(".link-target/guide-link.md"),
    )?;

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("#missing"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_markdown_files_during_discovery() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write("README.md", "# Home\n")?;
    fixture.write("docs/guide.md", "# Guide\n")?;
    std::os::unix::fs::symlink("docs/guide.md", fixture.path().join("guide-link.md"))?;

    let error = load(fixture.path())
        .err()
        .ok_or_else(|| io::Error::other("Markdown symlink unexpectedly entered the inventory"))?;

    assert!(error.contains("symbolic link"));
    assert!(error.contains("guide-link.md"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_markdown_files_outside_repository() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let outside = Fixture::new()?;
    fixture.write("README.md", "# Home\n")?;
    outside.write("guide.md", "# Outside guide\n")?;
    std::os::unix::fs::symlink(
        outside.path().join("guide.md"),
        fixture.path().join("outside-guide.md"),
    )?;

    let error = load(fixture.path())
        .err()
        .ok_or_else(|| io::Error::other("external Markdown symlink was not rejected"))?;

    assert!(error.contains("symbolic link"));
    assert!(error.contains("outside-guide.md"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_directories_during_discovery() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write("README.md", "# Home\n")?;
    fixture.write("docs/guide.md", "# Guide\n")?;
    std::os::unix::fs::symlink("docs", fixture.path().join("guides"))?;

    let error = load(fixture.path())
        .err()
        .ok_or_else(|| io::Error::other("symlinked Markdown directory was not rejected"))?;

    assert!(error.contains("symbolic link"));
    assert!(error.contains("guides"));
    Ok(())
}

#[test]
fn rejects_incorrect_case_repository_escape_and_machine_paths() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write(
        "README.md",
        "# Home\n\n[case](Docs/Guide.md)\n[escape](../outside.md)\n\
         [drive](C:/Source/guide.md)\n[file](file:///tmp/guide.md)\n",
    )?;
    fixture.write("docs/guide.md", "# Guide\n")?;
    let repository = load_fixture(fixture.path())?;

    assert_eq!(check_links(&repository).len(), 4);
    Ok(())
}

#[test]
fn validates_root_readme_fragments_after_parent_traversal() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write("README.md", "# Home\n")?;
    fixture.write(
        "docs/guide.md",
        "# Guide\n\n[valid](../#home)\n[broken](../#missing)\n",
    )?;
    let repository = load_fixture(fixture.path())?;

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("#missing"));
    assert!(violations[0].message.contains("README.md"));
    Ok(())
}

#[test]
fn rejects_excluded_directory_links_but_allows_new_files() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write(
        "README.md",
        "# Home\n\n[internal](.git/config)\n[new](drafts/new.md#new-guide)\n",
    )?;
    fixture.write(".git/config", "fixture configuration\n")?;
    let repository = load_fixture(fixture.path())?;
    fixture.write("drafts/new.md", "# New guide\n")?;

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("excluded directory `.git`"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlink_aliases_into_excluded_directories() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write("README.md", "# Home\n\n[internal](config-link)\n")?;
    fixture.write(".git/config", "fixture configuration\n")?;
    std::os::unix::fs::symlink(".git/config", fixture.path().join("config-link"))?;
    let repository = load_fixture(fixture.path())?;

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1);
    assert!(
        violations[0]
            .message
            .contains("resolves through excluded directory `.git`")
    );
    Ok(())
}

#[test]
fn rejects_renderer_incorrect_leading_slash_links() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write(
        "README.md",
        "# Home\n\n[root](/docs/guide.md)\n[encoded](%2Fdocs/guide.md)\n[external](//example.com)\n",
    )?;
    fixture.write("docs/guide.md", "# Guide\n")?;
    let repository = load_fixture(fixture.path())?;

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 2);
    assert!(
        violations
            .iter()
            .all(|violation| violation.message.contains("source-relative"))
    );
    Ok(())
}

#[test]
fn rejects_heading_fragments_for_non_markdown_targets() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write(
        "README.md",
        "# Home\n\n[file](Cargo.toml#missing)\n[directory](assets#missing)\n",
    )?;
    fixture.write("Cargo.toml", "[workspace]\n")?;
    fixture.write("assets/data.txt", "fixture\n")?;
    let repository = load_fixture(fixture.path())?;

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 2);
    assert!(
        violations
            .iter()
            .all(|violation| { violation.message.contains("targets non-Markdown path") })
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn reports_deterministic_case_match_when_names_collide() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write("README.md", "# Home\n\n[case](docs/FOO.md)\n")?;
    fixture.write("docs/Foo.md", "# Uppercase guide\n")?;
    fixture.write("docs/foo.md", "# Lowercase guide\n")?;
    let repository = load_fixture(fixture.path())?;

    let violations = check_links(&repository);

    assert_eq!(violations.len(), 1);
    assert!(
        violations[0]
            .message
            .contains("filesystem entry is `Foo.md`")
    );
    Ok(())
}

#[test]
fn tldr_policy_uses_visible_content_and_includes_adrs_and_product_guides()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.write("long.md", &format!("# Long\n\n{}\n", "word ".repeat(800)))?;
    fixture.write("docs/architecture/short.md", "# Architecture\n\nShort.\n")?;
    fixture.write("docs/product/short.md", "# Product\n\nShort.\n")?;
    fixture.write(
        "docs/architecture/compliant.md",
        "# Architecture\n\n## TL;DR\n\nShort.\n",
    )?;
    fixture.write(
        "docs/architecture/late-summary.md",
        "# Architecture\n\nBackground before the summary.\n\n## TL;DR\n\nShort.\n",
    )?;
    fixture.write(
        "docs/architecture/hidden-summary.md",
        "# Architecture\n\n## <span hidden>TL;DR</span>\n\nShort.\n",
    )?;
    fixture.write(
        "docs/architecture/raw-before-summary.md",
        "# Architecture\n\n<div>Background.</div>\n\n## TL;DR\n\nShort.\n",
    )?;
    fixture.write(
        "docs/architecture/image-before-summary.md",
        "# Architecture\n\n![Background](image.png)\n\n## TL;DR\n\nShort.\n",
    )?;
    fixture.write(
        "docs/architecture/code-before-summary.md",
        "# Architecture\n\n```text\nBackground.\n```\n\n## TL;DR\n\nShort.\n",
    )?;
    fixture.write(
        "docs/architecture/empty-summary.md",
        "# Architecture\n\n## TL;DR\n\n## Context\n\nDetails.\n",
    )?;
    fixture.write(
        "docs/architecture/raw-html-summary-body.md",
        "# Architecture\n\n## TL;DR\n\n<span hidden>\nHidden summary.\n</span>\n",
    )?;
    fixture.write(
        "docs/architecture/adr-metadata-before-summary.md",
        "# Architecture\n\n- Date: 2026-09-29\n\n## TL;DR\n\nSummary.\n",
    )?;
    fixture.write(
        "docs/architecture/status-before-summary.md",
        "# Architecture\n\nStatus: Draft\n\n## TL;DR\n\nSummary.\n",
    )?;
    fixture.write(
        "docs/architecture/prose-before-title.md",
        "Narrative before the title.\n\n# Architecture\n\n## TL;DR\n\nSummary.\n",
    )?;
    fixture.write(
        "docs/architecture/comment-before-title.md",
        "<!-- author note -->\n\n# Architecture\n\n## TL;DR\n\nSummary.\n",
    )?;
    fixture.write(
        "docs/architecture/raw-html-title.md",
        "# <span>Architecture</span>\n\n## TL;DR\n\nSummary.\n",
    )?;
    fixture.write(
        "docs/architecture/image-title.md",
        "# ![Badge](badge.png) Architecture\n\n## TL;DR\n\nSummary.\n",
    )?;
    fixture.write(
        "docs/architecture/setext-title.md",
        "Architecture\n============\n\n## TL;DR\n\nSummary.\n",
    )?;
    fixture.write(
        "inline-code-long.md",
        &format!("# Inline code guide\n\n{}\n", "`token` ".repeat(800)),
    )?;
    fixture.write(
        "docs/decisions/0001-long.md",
        &format!("# ADR-0001: Long\n\n{}\n", "word ".repeat(800)),
    )?;
    fixture.write(
        "docs/decisions/0002-compliant.md",
        "# ADR-0002: Compliant\n\n- Status: Accepted\n- Date: 2026-09-29\n\
         - Milestone: M01 - Rust engineering foundation\n- Deciders: Maintainers\n\
         - Supersedes:\n- Superseded by:\n\n## TL;DR\n\nSummary.\n",
    )?;
    fixture.write(
        "docs/decisions/README.md",
        &format!(
            "# Decisions\n\n- Status: Ignore the summary.\n\n## TL;DR\n\nSummary.\n\n{}\n",
            "word ".repeat(800)
        ),
    )?;
    fixture.write(
        ".github/ISSUE_TEMPLATE.md",
        &format!("# Template\n\n{}\n", "word ".repeat(800)),
    )?;
    fixture.write(
        ".github/security-guide.md",
        &format!("# Security\n\n{}\n", "word ".repeat(800)),
    )?;
    fixture.write(
        "docs/development/security-review.md",
        "# Security review\n\nShort.\n",
    )?;
    let repository = load_fixture(fixture.path())?;

    let violations = check_tldr(&repository);
    let violated_paths: Vec<_> = violations.iter().map(|item| item.path.as_str()).collect();

    assert_eq!(violations.len(), 20);
    assert!(violated_paths.contains(&"docs/architecture/raw-html-summary-body.md"));
    assert!(violated_paths.contains(&"docs/architecture/adr-metadata-before-summary.md"));
    assert!(violated_paths.contains(&"docs/architecture/status-before-summary.md"));
    assert!(violated_paths.contains(&"docs/architecture/prose-before-title.md"));
    assert!(!violated_paths.contains(&"docs/architecture/comment-before-title.md"));
    assert!(violated_paths.contains(&"docs/architecture/raw-html-title.md"));
    assert!(violated_paths.contains(&"docs/architecture/image-title.md"));
    assert!(!violated_paths.contains(&"docs/architecture/setext-title.md"));
    assert!(violated_paths.contains(&"inline-code-long.md"));
    assert!(violated_paths.contains(&"docs/decisions/0001-long.md"));
    assert!(violated_paths.contains(&"docs/product/short.md"));
    assert!(!violated_paths.contains(&"docs/decisions/0002-compliant.md"));
    assert!(violated_paths.contains(&"docs/decisions/README.md"));
    assert!(!violated_paths.contains(&".github/ISSUE_TEMPLATE.md"));
    Ok(())
}

fn load_fixture(path: &Path) -> Result<Repository, Box<dyn Error>> {
    load(path).map_err(|message| io::Error::other(message).into())
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> io::Result<Self> {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "edgeagent-markdown-check-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, relative: &str, contents: &str) -> io::Result<()> {
        let path = self.root.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("fixture file must have a parent"))?;
        fs::create_dir_all(parent)?;
        fs::write(path, contents)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
