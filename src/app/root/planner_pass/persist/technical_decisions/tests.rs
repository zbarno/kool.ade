use super::*;

fn record(question_id: &str, decision: &str, rationale: &str) -> String {
    format!(
        "<!-- koolade-clarification-resolution:{question_id} -->\n- **Repository-evidenced technical decision {question_id}:** {decision}\n  - Rationale: {rationale}\n  - Evidence: repository-overview:feature\n"
    )
}

#[test]
fn existing_identical_decision_is_idempotent() {
    let expected = record("question-id", "Keep JSON", "README precedent");
    let mut markdown = format!("## Decisions and Assumptions\n\n{expected}");
    append_or_verify(&mut markdown, &expected).unwrap();
    assert_eq!(
        markdown
            .matches("koolade-clarification-resolution:")
            .count(),
        1
    );
}

#[test]
fn conflicting_marker_fails_closed_without_changing_the_specification() {
    let existing = record("question-id", "Use XML", "A different decision");
    let expected = record("question-id", "Keep JSON", "README precedent");
    let mut markdown = format!("## Decisions and Assumptions\n\n{existing}");
    let before = markdown.clone();
    assert!(append_or_verify(&mut markdown, &expected).is_err());
    assert_eq!(markdown, before);
}

#[test]
fn duplicate_markers_fail_closed() {
    let expected = record("question-id", "Keep JSON", "README precedent");
    let mut markdown = format!("{expected}\n{expected}");
    assert!(append_or_verify(&mut markdown, &expected).is_err());
}
