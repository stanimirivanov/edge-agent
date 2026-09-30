use std::collections::{BTreeMap, BTreeSet};

use pulldown_cmark::{
    CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd,
};

use super::{Document, Heading, MarkdownCodeBlock, MarkdownLink, MarkdownTableRow};

pub(super) fn parse(relative_path: String, body: String) -> Document {
    let line_index = LineIndex::new(&body);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(&body, options);
    let definitions = parser
        .reference_definitions()
        .iter()
        .map(|(_, definition)| MarkdownLink {
            line: line_index.line(definition.span.start),
            destination: definition.dest.to_string(),
        })
        .collect();
    let mut builder = DocumentBuilder::new(relative_path, definitions);

    for (event, range) in parser.into_offset_iter() {
        let start_line = line_index.line(range.start);
        let end_line = line_index.line(range.end.saturating_sub(1));
        builder.consume(event, start_line, end_line);
    }

    builder.finish(body)
}

struct DocumentBuilder {
    relative_path: String,
    headings: Vec<Heading>,
    links: Vec<MarkdownLink>,
    code_blocks: Vec<MarkdownCodeBlock>,
    table_rows: Vec<MarkdownTableRow>,
    word_count: usize,
    code_block_depth: usize,
    image_depth: usize,
    html_comment_open: bool,
    raw_html_lines: BTreeSet<usize>,
    image_lines: BTreeSet<usize>,
    current_heading: Option<PendingHeading>,
    current_code_block: Option<MarkdownCodeBlock>,
    current_table_row: Option<MarkdownTableRow>,
    current_table: Option<usize>,
    next_table: usize,
    table_cell_open: bool,
    used_anchors: BTreeSet<String>,
    visible_lines: BTreeMap<usize, String>,
}

impl DocumentBuilder {
    fn new(relative_path: String, links: Vec<MarkdownLink>) -> Self {
        Self {
            relative_path,
            headings: Vec::new(),
            links,
            code_blocks: Vec::new(),
            table_rows: Vec::new(),
            word_count: 0,
            code_block_depth: 0,
            image_depth: 0,
            html_comment_open: false,
            raw_html_lines: BTreeSet::new(),
            image_lines: BTreeSet::new(),
            current_heading: None,
            current_code_block: None,
            current_table_row: None,
            current_table: None,
            next_table: 0,
            table_cell_open: false,
            used_anchors: BTreeSet::new(),
            visible_lines: BTreeMap::new(),
        }
    }

    fn consume(&mut self, event: Event<'_>, line: usize, end_line: usize) {
        match event {
            Event::Start(Tag::Heading { level, id, .. }) => {
                self.start_heading(level, id, line);
            }
            Event::End(TagEnd::Heading(_)) => self.finish_heading(end_line),
            Event::Start(Tag::Table(_)) => self.start_table(),
            Event::End(TagEnd::Table) => self.current_table = None,
            Event::Start(Tag::TableHead) => self.start_table_row(line, true),
            Event::Start(Tag::TableRow) => self.start_table_row(line, false),
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                if let Some(row) = self.current_table_row.take() {
                    self.table_rows.push(row);
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(row) = &mut self.current_table_row {
                    row.cells.push(String::new());
                    self.table_cell_open = true;
                }
            }
            Event::End(TagEnd::TableCell) => self.table_cell_open = false,
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) if !is_reference(link_type) => {
                self.links.push(MarkdownLink {
                    line,
                    destination: dest_url.into_string(),
                });
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                ..
            }) => {
                self.image_depth += 1;
                self.image_lines.insert(line);
                if !is_reference(link_type) {
                    self.links.push(MarkdownLink {
                        line,
                        destination: dest_url.into_string(),
                    });
                }
            }
            Event::End(TagEnd::Image) => self.image_depth -= 1,
            Event::Start(Tag::CodeBlock(kind)) => {
                self.start_code_block(kind, line);
            }
            Event::End(TagEnd::CodeBlock) => self.finish_code_block(),
            Event::Text(text) if self.code_block_depth > 0 => {
                if let Some(block) = &mut self.current_code_block {
                    block.body.push_str(&text);
                }
            }
            Event::InlineHtml(html) | Event::Html(html) if self.code_block_depth == 0 => {
                self.observe_html(line, &html);
            }
            Event::Text(text) if self.code_block_depth == 0 => {
                if let Some(heading) = &mut self.current_heading {
                    heading.anchor_text.push_str(&text);
                    if self.image_depth == 0 {
                        heading.visible_text.push_str(&text);
                    }
                }
                if self.policy_text_is_visible() {
                    self.mark_visible(line, &text);
                    self.word_count += word_count(&text);
                    self.append_table_text(&text);
                }
            }
            Event::Code(code) if self.code_block_depth == 0 => {
                if let Some(heading) = &mut self.current_heading {
                    heading.anchor_text.push_str(&code);
                    if self.image_depth == 0 {
                        heading.visible_text.push_str(&code);
                    }
                }
                if self.policy_text_is_visible() {
                    self.mark_visible(line, &code);
                    self.word_count += word_count(&code);
                    self.append_table_text(&code);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(heading) = &mut self.current_heading {
                    heading.anchor_text.push(' ');
                    if self.image_depth == 0 {
                        heading.visible_text.push(' ');
                    }
                }
                if self.policy_text_is_visible() {
                    self.append_table_text(" ");
                }
            }
            _ => {}
        }
    }

    fn start_heading(&mut self, level: HeadingLevel, id: Option<CowStr<'_>>, line: usize) {
        self.current_heading = Some(PendingHeading {
            level: heading_level(level),
            line,
            anchor_text: String::new(),
            visible_text: String::new(),
            explicit_id: id.map(CowStr::into_string),
        });
    }

    fn start_code_block(&mut self, kind: CodeBlockKind<'_>, line: usize) {
        self.code_block_depth += 1;
        self.current_code_block = Some(MarkdownCodeBlock {
            line,
            info: match kind {
                CodeBlockKind::Indented => String::new(),
                CodeBlockKind::Fenced(info) => info.into_string(),
            },
            body: String::new(),
        });
    }

    fn start_table(&mut self) {
        self.current_table = Some(self.next_table);
        self.next_table += 1;
    }

    fn start_table_row(&mut self, line: usize, is_header: bool) {
        let table = self.current_table.unwrap_or_else(|| {
            let table = self.next_table;
            self.next_table += 1;
            self.current_table = Some(table);
            table
        });
        self.current_table_row = Some(MarkdownTableRow {
            line,
            cells: Vec::new(),
            table,
            is_header,
        });
    }

    fn finish_code_block(&mut self) {
        self.code_block_depth -= 1;
        if let Some(block) = self.current_code_block.take() {
            self.code_blocks.push(block);
        }
    }

    fn finish_heading(&mut self, end_line: usize) {
        let Some(pending) = self.current_heading.take() else {
            return;
        };
        let base_anchor = pending
            .explicit_id
            .unwrap_or_else(|| github_anchor(&pending.anchor_text));
        let mut anchor = base_anchor.clone();
        let mut suffix = 0;
        while !self.used_anchors.insert(anchor.clone()) {
            suffix += 1;
            anchor = format!("{base_anchor}-{suffix}");
        }
        self.headings.push(Heading {
            level: pending.level,
            line: pending.line,
            end_line,
            text: pending.visible_text,
            anchor,
        });
    }

    fn finish(self, body: String) -> Document {
        Document {
            relative_path: self.relative_path,
            body,
            headings: self.headings,
            links: self.links,
            code_blocks: self.code_blocks,
            table_rows: self.table_rows,
            word_count: self.word_count,
            visible_lines: self.visible_lines,
            raw_html_lines: self.raw_html_lines,
            image_lines: self.image_lines,
        }
    }

    fn mark_visible(&mut self, first_line: usize, text: &str) {
        for (offset, fragment) in text.split('\n').enumerate() {
            self.visible_lines
                .entry(first_line + offset)
                .or_default()
                .push_str(fragment);
        }
    }

    fn append_table_text(&mut self, text: &str) {
        if self.table_cell_open
            && let Some(cell) = self
                .current_table_row
                .as_mut()
                .and_then(|row| row.cells.last_mut())
        {
            cell.push_str(text);
        }
    }

    fn policy_text_is_visible(&self) -> bool {
        self.image_depth == 0
    }

    fn observe_html(&mut self, line: usize, html: &str) {
        let mut remaining = html;
        loop {
            if self.html_comment_open {
                let Some(end) = remaining.find("-->") else {
                    return;
                };
                self.html_comment_open = false;
                remaining = &remaining[end + 3..];
                continue;
            }

            remaining = remaining.trim_start();
            if remaining.is_empty() {
                return;
            }
            if let Some(comment) = remaining.strip_prefix("<!--") {
                self.html_comment_open = true;
                remaining = comment;
                continue;
            }

            self.raw_html_lines.insert(line);
            return;
        }
    }
}

fn is_reference(link_type: LinkType) -> bool {
    matches!(
        link_type,
        LinkType::Reference
            | LinkType::ReferenceUnknown
            | LinkType::Collapsed
            | LinkType::CollapsedUnknown
            | LinkType::Shortcut
            | LinkType::ShortcutUnknown
    )
}

struct PendingHeading {
    level: u8,
    line: usize,
    anchor_text: String,
    visible_text: String,
    explicit_id: Option<String>,
}

struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(body: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            body.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self { starts }
    }

    fn line(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset)
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn word_count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .count()
}

fn github_anchor(text: &str) -> String {
    text.trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter_map(|character| match character {
            ' ' => Some('-'),
            '-' | '_' => Some(character),
            character if character.is_alphanumeric() => Some(character),
            _ => None,
        })
        .collect()
}
