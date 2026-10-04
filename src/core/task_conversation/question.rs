//! Constrained response contract for a Question task.

pub(super) fn build(context: &str, history: &str, message: &str) -> String {
    format!(
        "Answer this Question task directly using repository evidence. Inspect relevant source and project documents before reaching a conclusion. Cite paths or named product decisions in the answer, explain uncertainty, and distinguish observed behavior from recommendations. Do not create or update specifications, interview briefs, task stories, or project actions. If the investigation uncovers a genuine choice that only the user can make, create a Human board item with clear alternatives, a concrete consequence for each, and an advisory recommendation. When the user asks why a product capability is unsupported or unavailable and a feasible product change could enable it, answer the question and offer (never create automatically) that related Feature task using follow_up_task with a concise title and description; state the choice in assistant_message. For questions about an unsupported hosted model provider, this offer takes precedence over architecture debate: do not create an open item about whether to pursue the change. Explain the current limitation, include the exact follow_up_task offer, and let the user decide whether to create work by selecting it. In particular, “Why can't Kool.ad/e use hosted Anthropic through Pi?” must set follow_up_task to an offer such as {{\"title\":\"Add hosted-provider support\",\"description\":\"Plan safe hosted model provider support through Pi.\"}}; do not substitute a workaround or review item for this opt-in offer. Keep open_items_added and open_items_updated empty for this provider-support question. Otherwise answer and let this Question task finish. Return one fenced JSON object with schema_version 2 and these fields: assistant_message, change_summary, document_updates (empty array), open_items_added, open_items_updated, open_items_resolved, next_question_id (null unless it targets this task), requested_action (null), and follow_up_task (null unless offering user-consented work). The envelope field task_stories is unsupported here: never emit it; offering work uses only follow_up_task. Never write files directly.\n\nTASK CONTEXT\n{context}\n\nTASK HISTORY\n{history}\n\nUSER MESSAGE\n{message}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_prompt_requires_a_direct_evidence_based_answer_without_specs() {
        let prompt = build("Why does sign-in use SQLite?", "", "Why SQLite?");
        assert!(prompt.contains("Inspect relevant source and project documents"));
        assert!(prompt.contains("Do not create or update specifications"));
        assert!(prompt.contains("concrete consequence for each"));
        assert!(prompt.contains("assistant_message, change_summary"));
        assert!(prompt.contains("this offer takes precedence over architecture debate"));
        assert!(prompt.contains("Keep open_items_added and open_items_updated empty"));
    }
}
