//! Shared parsing for versioned EdgeAgent message type names.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MessageTypeParts<'value> {
    pub(crate) domain: &'value str,
    pub(crate) name: &'value str,
    pub(crate) version: &'value str,
}

pub(crate) fn parse_message_type(value: &str) -> Option<MessageTypeParts<'_>> {
    let mut segments = value.split('.');
    if segments.next() != Some("com") || segments.next() != Some("edgeagent") {
        return None;
    }
    let domain = segments.next()?;
    let name = segments.next()?;
    let version = segments.next()?;
    if segments.next().is_some() || !is_lowercase_token(domain) || !is_lowercase_token(name) {
        return None;
    }
    let major = version.strip_prefix('v')?;
    if major.is_empty()
        || major.starts_with('0')
        || !major.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(MessageTypeParts {
        domain,
        name,
        version,
    })
}

pub(crate) fn is_lowercase_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::{MessageTypeParts, parse_message_type};

    #[test]
    fn parser_returns_routing_segments() {
        assert_eq!(
            parse_message_type("com.edgeagent.research.artifact-published.v1"),
            Some(MessageTypeParts {
                domain: "research",
                name: "artifact-published",
                version: "v1",
            })
        );
    }

    #[test]
    fn parser_rejects_extra_segments_and_zero_major_version() {
        assert!(parse_message_type("com.edgeagent.research.artifact.published.v1").is_none());
        assert!(parse_message_type("com.edgeagent.research.artifact-published.v0").is_none());
    }
}
