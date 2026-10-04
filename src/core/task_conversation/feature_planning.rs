//! Strict response contract for focused planning-feature conversations.

pub(super) fn build(
    context: &str,
    current_brief: &str,
    history: &str,
    message: &str,
    next_feature_id: String,
) -> String {
    format!(
        "Continue this typed planning task using the validated Kool.ad/e document and board-item contract. Resolve Agent-owned questions from evidence. Record every currently actionable Human/Review decision as its own board item, including concrete consequences and recommendation rationale; create independent items together and set blocked_by for dependent choices that must wait. Never leave an actionable question only in assistant_message. Compare each proposed item with supplied open items and items in this response; never create a second item for the same decision under different wording or scope. When multiple independent items are on the board, say they can be answered in any order; do not imply that one gates the others. Keep next_question_id null for new items and do not ask the user in assistant_message to answer only one independent item. Record only unresolved questions, ambiguities, or assumptions as new items. Do not create a new item merely to restate an approved decision or an assumption already recorded in the feature. Keep each specification concise: write each fact once in its best-fit section, omit generic boilerplate, and do not copy the user request as a second specification. Next feature ID for genuinely new work: {next_feature_id}. Never write files directly.\n\
         Return exactly one fenced JSON object with schema_version 2 and only these top-level fields: assistant_message, change_summary, document_updates, open_items_added, open_items_updated, open_items_resolved, next_question_id, requested_action, and interview when updating the task plan. Do not wrap the response in mode, phase, result, updated_specification, or other workflow fields. Each document_updates entry must contain document_id and the full replacement content; include typed status only for an intentional feature lifecycle transition. open_items_resolved must be an array of ID strings, never item objects. Use empty arrays for unchanged collections and requested_action null when no application action is requested. To enable task generation after finishing a feature, also return the full interview object with featureName, problem, goal, targetUsers, intendedOutcome, successCriteria, inScope, outOfScope, constraints, readyForTasks=true; set featureName to this feature and preserve its approved scope.\n\
         === FEATURE CONTEXT ===\n{context}\n\
         === CURRENT TASK PLAN (interview JSON) ===\n{current_brief}\n\
         === THIS FEATURE CONVERSATION ===\n{history}\n\
         === USER REPLY ===\n{message}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_planning_prompt_requires_the_application_wire_shape() {
        let prompt = build("Feature F7", "{}", "", "Set status to Ready", "F8".into());
        assert!(prompt.contains("schema_version 2"));
        assert!(prompt.contains("only these top-level fields"));
        assert!(prompt.contains("full replacement content"));
        assert!(prompt.contains("interview when updating the task plan"));
        assert!(prompt.contains("readyForTasks=true"));
        assert!(prompt.contains("open_items_resolved must be an array of ID strings"));
        assert!(prompt.contains("Do not wrap the response in mode, phase, result"));
        assert!(prompt.contains("Record only unresolved questions, ambiguities, or assumptions"));
        assert!(prompt.contains("Do not create a new item merely to restate an approved decision"));
        assert!(prompt.contains("never create a second item for the same decision"));
        assert!(prompt.contains(
            "do not ask the user in assistant_message to answer only one independent item"
        ));
        assert!(!prompt.contains("updated_specification as a top-level field"));
    }
}
