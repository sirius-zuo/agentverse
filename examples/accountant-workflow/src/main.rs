fn main() {}

/// Strips NEXT_SKILL and SUMMARY directives from LLM output, returning the deliverable body.
/// Mirrors the internal logic of `parse_phase_transition` in avs-agent.
pub fn extract_deliverable(output: &str) -> String {
    let mut lines: Vec<&str> = output.trim_end().lines().collect();
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    if lines.last().map(|l| l.trim().starts_with("SUMMARY:")).unwrap_or(false) {
        lines.pop();
    }
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    if lines.last().map(|l| l.trim().starts_with("NEXT_SKILL:")).unwrap_or(false) {
        lines.pop();
    }
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_both_directives() {
        let output = "Transactions:\n- Rent: -$2000\n- Payment: +$5000\n\nNEXT_SKILL: prepare-journal-entry\nSUMMARY: Found 2 transactions";
        assert_eq!(
            extract_deliverable(output),
            "Transactions:\n- Rent: -$2000\n- Payment: +$5000"
        );
    }

    #[test]
    fn no_directives_returns_unchanged() {
        let output = "Ledger posted. Confirmation: CONF-JE-2026-06.";
        assert_eq!(extract_deliverable(output), output);
    }

    #[test]
    fn handles_trailing_blank_lines_around_directives() {
        let output = "Body text\n\nNEXT_SKILL: foo\nSUMMARY: bar\n\n";
        assert_eq!(extract_deliverable(output), "Body text");
    }
}
