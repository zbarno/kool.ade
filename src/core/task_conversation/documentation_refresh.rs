//! Repository survey and documentation-refresh contract for a typed task.

pub(super) fn build(context: &str, history: &str, message: &str) -> String {
    format!(
        "Analyze this existing repository and create or refresh its Kool.ad/e project documentation. Start with repo-level documentation (README, docs, manifests, contribution/deployment notes), then inspect source to verify claims and document actual behavior. Inspect the canonical .koolade-packet planning/product documents; treat them as complete only when their coverage matches the repository. If the artifact directory is absent or incomplete, create the useful product documents needed to describe purpose, scope, architecture, important workflows, data, environment, risks, decisions, and source map. Base every statement on repository evidence, cite paths in the document, preserve supported existing facts, and label uncertainty. Work through coherent bounded slices and continue until major modules and user-visible behavior are covered; do not fill unknowns with boilerplate. Never change application source or automatically fix findings.\n\
         Raise unresolved questions, contradictions between documentation and code, and decisions requiring the user's knowledge as individual Question tasks with status NeedsAttention. Raise suspected bugs, vulnerabilities, and other quality concerns as Bug tasks with status Todo. Each task description must identify evidence paths, observed facts, why the finding is uncertain, and the concrete triage step; do not state an unverified concern as a confirmed defect. Avoid duplicates against existing planning tasks.\n\
         Return exactly one fenced JSON object with schema_version 2 and only these top-level fields: assistant_message, change_summary, document_updates, planning_tasks, open_items_added, open_items_updated, open_items_resolved, next_question_id, requested_action. Do not return interview, task_stories, task_outline, updated_specification, or requested actions. Use full replacement content for each document_updates entry, with a valid product document_id and no lifecycle status. Keep planning_tasks entries to title, description, kind, and status; use kind `question` with status `needs_attention` for unresolved questions or contradictions, and kind `bug` with status `todo` for suspected defects and vulnerabilities. Use empty arrays for unchanged collections and null for next_question_id/requested_action when unused. Never write repository files directly.\n\
         === TASK CONTEXT ===\n{context}\n\
         === CONVERSATION SO FAR ===\n{history}\n\
         === USER MESSAGE ===\n{message}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_prompt_separates_attention_questions_from_todo_triage() {
        let prompt = build("context", "", "refresh");
        assert!(prompt.contains("status NeedsAttention"));
        assert!(prompt.contains("status `todo`"));
        assert!(prompt.contains("kind `question`"));
        assert!(prompt.contains("kind `bug`"));
        assert!(prompt.contains("Never change application source or automatically fix findings"));
        assert!(prompt.contains("evidence paths"));
        assert!(prompt.contains("planning_tasks"));
    }
}
