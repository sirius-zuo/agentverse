// avs-guardrails/src/output_guard.rs
use regex::Regex;
use std::sync::LazyLock;

use crate::GuardrailError;

static PII_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(), "SSN"),
        (
            Regex::new(r"\b\d{4}[\s-]\d{4}[\s-]\d{4}[\s-]\d{4}\b").unwrap(),
            "Credit Card",
        ),
        (
            Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
            "Email",
        ),
    ]
});

/// Check if output contains PII or sensitive data.
pub fn check_output(output: &str) -> Result<(), GuardrailError> {
    for (pattern, pii_type) in PII_PATTERNS.iter() {
        if pattern.is_match(output) {
            return Err(GuardrailError::OutputFiltered(format!(
                "PII detected: {} — output filtered",
                pii_type
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_output() {
        assert!(check_output("Hello, this is a clean response").is_ok());
    }

    #[test]
    fn test_ssn_detection() {
        let result = check_output("Your SSN is 123-45-6789");
        assert!(result.is_err());
    }

    #[test]
    fn test_email_detection() {
        let result = check_output("Contact us at test@example.com");
        assert!(result.is_err());
    }
}
