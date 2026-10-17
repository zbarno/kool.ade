use super::*;

#[test]
fn superseded_question_markdown_keeps_its_recorded_decision() {
    let mut question = propose_questions(vec![super::proposal("data owner", "Who owns it?")], &[])
        .unwrap()
        .remove(0);
    reconcile_answer(
        &mut question,
        0,
        4,
        Some("Keep the existing owner."),
        None,
        "operator",
        10,
    )
    .unwrap();
    supersede_question(&mut question, 1, 5).unwrap();

    let markdown = super::super::questions::render_markdown(&question).unwrap();
    let markdown = String::from_utf8(markdown).unwrap();
    assert!(markdown.contains("## Recorded decision"));
    assert!(markdown.contains("Keep the existing owner."));
    assert!(markdown.contains("Answered by: operator"));
}
