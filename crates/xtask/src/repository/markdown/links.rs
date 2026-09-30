use std::{ffi::OsString, fs, path::PathBuf};

use percent_encoding::percent_decode_str;

use super::{Document, REPOSITORY_POLICY, is_excluded_directory};
use crate::repository::{Repository, Violation};

pub(super) fn validate(repository: &Repository, document: &Document) -> Vec<Violation> {
    document
        .links
        .iter()
        .filter_map(|link| validate_link(repository, document, link))
        .collect()
}

fn validate_link(
    repository: &Repository,
    document: &Document,
    link: &super::MarkdownLink,
) -> Option<Violation> {
    let destination = link.destination.trim();
    if destination.is_empty() {
        return None;
    }
    if is_machine_absolute(destination) {
        return Some(violation(
            document,
            link.line,
            destination,
            "local Markdown destinations cannot use machine-specific absolute paths",
        ));
    }
    if is_external(destination) {
        return None;
    }

    let (path_part, fragment) = split_destination(destination);
    let decoded_path =
        decode(path_part).map_err(|message| violation(document, link.line, destination, message));
    let decoded_fragment = fragment
        .map(decode)
        .transpose()
        .map_err(|message| violation(document, link.line, destination, message));
    let (decoded_path, decoded_fragment) = match (decoded_path, decoded_fragment) {
        (Ok(path), Ok(fragment)) => (path, fragment),
        (Err(violation), _) | (_, Err(violation)) => return Some(violation),
    };

    let relative_target = if decoded_path.is_empty() {
        PathBuf::from(&document.relative_path)
    } else {
        if decoded_path.starts_with('/') {
            return Some(violation(
                document,
                link.line,
                destination,
                "local Markdown destinations must be source-relative and cannot start with `/`",
            ));
        }
        match resolve_relative_path(&document.relative_path, &decoded_path) {
            Ok(path) => path,
            Err(message) => return Some(violation(document, link.line, destination, message)),
        }
    };
    if let Some(excluded) = excluded_directory(&relative_target) {
        return Some(violation(
            document,
            link.line,
            destination,
            format!(
                "local Markdown destinations cannot traverse excluded directory `{}`",
                excluded.to_string_lossy()
            ),
        ));
    }
    let target = match exact_case_target(&repository.root, &relative_target) {
        Ok(target) => target,
        Err(message) => return Some(violation(document, link.line, destination, message)),
    };

    let fragment = decoded_fragment.filter(|fragment| !fragment.is_empty())?;
    let target_key = target_key(&repository.root, &target);
    let anchor_exists = if let Some(target_document) = repository.documents.get(&target_key) {
        target_document
            .headings
            .iter()
            .any(|heading| heading.anchor == fragment)
    } else if is_markdown_target(&target) {
        let body = match fs::read_to_string(&target) {
            Ok(body) => body,
            Err(error) => {
                return Some(violation(
                    document,
                    link.line,
                    destination,
                    format!("could not read Markdown target `{target_key}`: {error}"),
                ));
            }
        };
        super::parse_fragment(&target_key, &body)
            .headings
            .iter()
            .any(|heading| heading.anchor == fragment)
    } else {
        return Some(violation(
            document,
            link.line,
            destination,
            format!(
                "fragment `#{fragment}` targets non-Markdown path `{target_key}` and cannot name a rendered heading"
            ),
        ));
    };
    if anchor_exists {
        None
    } else {
        Some(violation(
            document,
            link.line,
            destination,
            format!("heading anchor `#{fragment}` does not exist in `{target_key}`"),
        ))
    }
}

fn is_markdown_target(target: &std::path::Path) -> bool {
    target
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn split_destination(destination: &str) -> (&str, Option<&str>) {
    let (without_fragment, fragment) = destination
        .split_once('#')
        .map_or((destination, None), |(path, fragment)| {
            (path, Some(fragment))
        });
    let path = without_fragment
        .split_once('?')
        .map_or(without_fragment, |(path, _)| path);
    (path, fragment)
}

fn decode(value: &str) -> Result<String, String> {
    if !has_valid_percent_encoding(value) {
        return Err(format!("`{value}` contains an incomplete percent escape"));
    }
    String::from_utf8(percent_decode_str(value).collect())
        .map_err(|_| format!("`{value}` contains invalid percent-encoded UTF-8"))
}

fn has_valid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        if bytes
            .get(index + 1..=index + 2)
            .is_none_or(|digits| !digits.iter().all(u8::is_ascii_hexdigit))
        {
            return false;
        }
        index += 3;
    }
    true
}

fn is_external(destination: &str) -> bool {
    if destination.starts_with("//") {
        return true;
    }
    let prefix = destination
        .split(['/', '#', '?'])
        .next()
        .unwrap_or_default();
    let Some((scheme, _)) = prefix.split_once(':') else {
        return false;
    };

    !scheme.is_empty()
        && scheme.chars().enumerate().all(|(index, character)| {
            if index == 0 {
                character.is_ascii_alphabetic()
            } else {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
            }
        })
}

fn is_machine_absolute(destination: &str) -> bool {
    let bytes = destination.as_bytes();
    destination
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

fn resolve_relative_path(source: &str, destination: &str) -> Result<PathBuf, String> {
    if destination.contains('\\') {
        return Err("local Markdown destinations must use `/` separators".to_owned());
    }
    let mut components: Vec<&str> = if destination.starts_with('/') {
        Vec::new()
    } else {
        source
            .rsplit_once('/')
            .map_or(Vec::new(), |(parent, _)| parent.split('/').collect())
    };

    for component in destination.trim_start_matches('/').split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err("local Markdown destination escapes the repository root".to_owned());
                }
            }
            component => components.push(component),
        }
    }

    Ok(components.iter().collect())
}

fn exact_case_target(
    root: &std::path::Path,
    relative: &std::path::Path,
) -> Result<PathBuf, String> {
    let mut current = root.to_path_buf();
    for expected in relative.components().map(std::path::Component::as_os_str) {
        let entries = fs::read_dir(&current)
            .map_err(|_| format!("local path `{}` does not exist", relative.display()))?;
        let mut names = entries
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("could not inspect `{}`: {error}", current.display()))?
            .into_iter()
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>();
        names.sort();
        if names.iter().any(|name| name == expected) {
            current.push(expected);
            continue;
        }
        let case_match = names.iter().find(|name| same_ignoring_case(name, expected));
        return Err(case_match.map_or_else(
            || format!("local path `{}` does not exist", relative.display()),
            |actual| {
                format!(
                    "local path `{}` has incorrect case; filesystem entry is `{}`",
                    relative.display(),
                    actual.to_string_lossy()
                )
            },
        ));
    }

    if current.is_dir() {
        let readme = current.join("README.md");
        if readme.is_file() {
            current = readme;
        }
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("could not resolve repository root: {error}"))?;
    let canonical_target = current
        .canonicalize()
        .map_err(|error| format!("could not resolve `{}`: {error}", relative.display()))?;
    let canonical_relative = canonical_target
        .strip_prefix(&canonical_root)
        .map_err(|_| {
            format!(
                "local path `{}` resolves outside the repository root",
                relative.display()
            )
        })?;
    if let Some(excluded) = excluded_directory(canonical_relative) {
        return Err(format!(
            "local path `{}` resolves through excluded directory `{}`",
            relative.display(),
            excluded.to_string_lossy()
        ));
    }
    Ok(current)
}

fn excluded_directory(path: &std::path::Path) -> Option<&std::ffi::OsStr> {
    path.components()
        .map(std::path::Component::as_os_str)
        .find(|component| is_excluded_directory(component))
}

fn same_ignoring_case(left: &OsString, right: &std::ffi::OsStr) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn target_key(root: &std::path::Path, target: &std::path::Path) -> String {
    target
        .strip_prefix(root)
        .unwrap_or(target)
        .to_string_lossy()
        .replace('\\', "/")
}

fn violation(
    document: &Document,
    line: usize,
    destination: &str,
    message: impl Into<String>,
) -> Violation {
    Violation::new(
        document.relative_path.clone(),
        line,
        "markdown.local-link",
        format!("invalid local link `{destination}`: {}", message.into()),
        "use an existing repository-relative path and exact GitHub heading anchor",
        REPOSITORY_POLICY,
    )
}
