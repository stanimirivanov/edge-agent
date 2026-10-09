//! Validated, static reason codes shared by inbound and outbound messaging.

/// A bounded reason code safe to retain as terminal message evidence.
///
/// Codes contain 1 to 64 bytes from `a-z`, `0-9`, `_`, or `-`. Construct them
/// as constants so invalid service-owned codes fail during compilation.
///
/// ```compile_fail
/// use edgeagent_messaging::FailureCode;
///
/// const INVALID: FailureCode = FailureCode::from_static("Bad Code");
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FailureCode(&'static str);

impl FailureCode {
    /// Validate a static reason code.
    ///
    /// # Panics
    ///
    /// Panics if `code` is empty, exceeds 64 bytes, or contains a byte other
    /// than lowercase ASCII letters, digits, `_`, or `-`. When called in a
    /// `const` declaration, invalid input fails compilation instead.
    #[must_use]
    pub const fn from_static(code: &'static str) -> Self {
        assert!(valid_failure_code(code), "invalid failure code");
        Self(code)
    }

    /// Return the validated code for persistence and diagnostic classification.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

const fn valid_failure_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        if !matches!(bytes[index], b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-') {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::FailureCode;

    const SINGLE: FailureCode = FailureCode::from_static("a");
    const MAXIMUM: FailureCode = FailureCode::from_static(concat!(
        "aaaaaaaaaaaaaaaa",
        "aaaaaaaaaaaaaaaa",
        "aaaaaaaaaaaaaaaa",
        "aaaaaaaaaaaaaaaa"
    ));

    #[test]
    fn accepts_inclusive_bounds_and_token_alphabet() {
        assert_eq!(SINGLE.as_str(), "a");
        assert_eq!(MAXIMUM.as_str().len(), 64);
        assert_eq!(FailureCode::from_static("a-z_09").as_str(), "a-z_09");
    }

    #[test]
    fn rejects_empty_oversized_and_non_token_values() {
        for invalid in [
            "",
            concat!(
                "aaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaaa"
            ),
            "Uppercase",
            "has space",
            "non_ascii_é",
        ] {
            assert!(std::panic::catch_unwind(|| FailureCode::from_static(invalid)).is_err());
        }
    }
}
