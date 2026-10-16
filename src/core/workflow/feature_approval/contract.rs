/// Freeze the normative feature contract, excluding mutable status and
/// repository-observation sections. A changed desired behavior needs approval again.
pub fn feature_contract(text: &str) -> String {
    let mut capture = false;
    let mut selected = String::new();
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            capture = [
                "Intent",
                "Desired Behavior",
                "Scope",
                "Requirements",
                "Decisions and Assumptions",
                "Acceptance Criteria",
                "Selected Plan",
            ]
            .contains(&heading);
        }
        if capture {
            selected.push_str(line);
            selected.push('\n');
        }
    }
    selected
}
