#[test]
fn decision_confidence_enum_is_spelled_as_exact_lowercase_wire_values() {
    let policy = crate::core::prompt::SYSTEM_INSTRUCTIONS;
    assert!(policy.contains("Serialize confidence.level using exactly one lowercase enum:"));
    assert!(policy.contains("`low`, `medium`, or `high`."));
    assert!(!policy.contains("`Low`, `Medium`, or `High`."));
    assert!(policy.contains(
        "Stop reading once the affected behavior and material uncertainty are grounded."
    ));
    assert!(
        policy.contains(
            "Do not sweep unrelated feature histories, resolved items, or product modules."
        )
    );
}

#[test]
fn decision_brief_requires_the_current_wire_shape() {
    let policy = crate::core::prompt::SYSTEM_INSTRUCTIONS;
    assert!(policy.contains("`confidence` is an object with"));
    assert!(policy.contains("Do not use legacy `description`,"));
    assert!(policy.contains("the brief's own `question` field is still required."));
    assert!(policy.contains("deferConsequence` to one concrete sentence of at most 240"));
    assert!(policy.contains("do not\nwrite or rewrite `product:decisions`"));
    assert!(policy.contains("For an unresolved Human item, keep `adrAssessment.create` false"));
    assert!(policy.contains("for a nontechnical person who does not know this\ncodebase"));
    assert!(
        policy
            .contains("Translate\ntechnical constraints into what they mean for the user's work.")
    );
    assert!(policy.contains("each option ID to a concise, unique value of at most 48 characters"));
    assert!(policy.contains("ASCII letters, digits, hyphens, or underscores"));
}

#[test]
fn new_feature_and_human_decision_have_complete_wire_examples() {
    let policy = crate::core::prompt::SYSTEM_INSTRUCTIONS;
    assert!(policy.contains("For a new feature, use the source map to select only relevant current product modules and decisions"));
    assert!(
        policy
            .contains("The application-assigned change-spec ID (for example `F7`) names a change")
    );
    assert!(policy.contains("separate from capability IDs such as `F-7`"));
    assert!(policy.contains("these eight exact H2 headings in this exact order"));
    assert!(policy.contains("Do not rename, paraphrase, omit, reorder, or add H2 headings"));
    assert!(
        policy.contains("when the application assigns a new ID such as `F7`, use `# F7: Title`")
    );
    assert!(policy.contains("a non-empty `assigned_to`"));
    assert!(policy.contains("Omit the new item's `id` so Kool.ad/e assigns its `CLR-nnn` ID"));
    assert!(policy.contains("an omitted `offset` starts again at the beginning of the file"));
    assert!(policy.contains("Never repeat a covered range to refresh it."));
    assert!(policy.contains("never stop at a prose clarification question for a new feature"));
    assert!(policy.contains("all of their cards now, set\n`next_question_id` null"));
    assert!(policy.contains("all listed cards can be\nanswered in any order"));
    assert!(
        policy.contains("Every newly added item must include these non-empty top-level fields")
    );
    assert!(
        policy.contains("Repeat the question in\nboth the item and `decision_brief.question`.")
    );
    assert!(policy.contains("leave\n`decision_brief.id` empty"));
    assert!(policy.contains("a prose-only answer saves nothing"));
    assert!(policy.contains("`document_updates` using"));
    assert!(policy.contains("`question` (never `text`)"));
    assert!(policy.contains("`assigned_to` (never `assignee`)"));
    assert!(policy.contains("\"deferConsequence\": \"What remains unresolved while waiting.\""));
    assert!(policy.contains("\"revisitWhen\": []"));
}
