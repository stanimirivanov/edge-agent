//! Restrict ambiguous YAML features before building a policy tree.

use yaml_rust2::{
    Yaml, YamlLoader,
    parser::{Event, Parser},
};

pub(super) fn workflow(source: &str) -> Result<Yaml, &'static str> {
    if source.len() > 512 * 1024 {
        return Err("workflow exceeds the 512 KiB input bound");
    }
    // Use the maintained parser's event API; never interpret YAML with a line scanner.
    // Checking before loading also prevents alias expansion and excessive nesting.
    let mut parser = Parser::new_from_str(source);
    let mut depth: usize = 0;
    let mut documents = 0;
    loop {
        let (event, _) = parser.next_token().map_err(|_| "malformed YAML workflow")?;
        match event {
            Event::Alias(_) => return Err("YAML aliases and anchors are not permitted"),
            Event::Scalar(_, _, anchor, tag) => {
                if anchor != 0 {
                    return Err("YAML aliases and anchors are not permitted");
                }
                if tag.is_some() {
                    return Err("explicit YAML tags are not permitted");
                }
            }
            Event::MappingStart(anchor, tag) | Event::SequenceStart(anchor, tag) => {
                if anchor != 0 {
                    return Err("YAML aliases and anchors are not permitted");
                }
                if tag.is_some() {
                    return Err("explicit YAML tags are not permitted");
                }
                depth += 1;
                if depth > 64 {
                    return Err("workflow YAML nesting exceeds 64 levels");
                }
            }
            Event::MappingEnd | Event::SequenceEnd => {
                depth = depth.saturating_sub(1);
            }
            Event::DocumentStart => {
                documents += 1;
                if documents > 1 {
                    return Err("workflow must contain exactly one YAML document");
                }
            }
            Event::StreamEnd => break,
            _ => {}
        }
    }
    if documents != 1 {
        return Err("workflow must contain exactly one YAML document");
    }
    // YamlLoader rejects duplicate mapping keys. Do not expose its value-bearing error text.
    let mut documents = YamlLoader::load_from_str(source)
        .map_err(|_| "malformed YAML or duplicate mapping keys")?;
    let root = documents
        .pop()
        .ok_or("workflow must contain exactly one YAML document")?;
    if root.as_hash().is_none() {
        return Err("workflow root must be a mapping");
    }
    if has_merge_key(&root) {
        return Err("YAML merge keys are not permitted");
    }
    Ok(root)
}

fn has_merge_key(node: &Yaml) -> bool {
    match node {
        Yaml::Hash(map) => map.iter().any(|(key, value)| {
            key.as_str() == Some("<<") || has_merge_key(key) || has_merge_key(value)
        }),
        Yaml::Array(values) => values.iter().any(has_merge_key),
        _ => false,
    }
}
