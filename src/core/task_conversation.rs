//! Focused prompts assembled from board identity and durable planning artifacts.
use crate::core::{context_build::clip, state::PlannerState};

mod documentation_refresh;
mod feature_planning;
mod references;

/// Immediate opening copy from the current in-memory board. Opening a chat
/// must not wait for a model or perform repository I/O on an input frame.
pub fn presentation(
    state: &PlannerState,
    docs: &[crate::artifacts::task_docs::TaskDocument],
    key: &str,
) -> Option<(String, String)> {
    if key.starts_with("planning:")
        || key.starts_with("feature:")
        || key.starts_with("task:")
        || key.starts_with("task-generation:")
    {
        let context = crate::core::planning_work::context(state, key)?;
        let kind = crate::core::planning_work::find(state, key)?.kind;
        let greeting = match kind {
            crate::core::planning_work::WorkKind::Question => {
                "Continue investigating this question here. Any user decision Kool.ad/e discovers will appear on the board."
            }
            crate::core::planning_work::WorkKind::TaskGeneration => {
                "Review this task's story-generation status here. Resolve the issue described in its details, then choose Generate tasks on the board to try again."
            }
            _ => {
                "Continue planning this task here. Questions and assumptions are tracked on the board."
            }
        };
        return Some((context, greeting.into()));
    }
    use crate::domain::{Authority, ItemStatus};
    let synthetic = crate::core::ownership::synthesize_for_state(state);
    let item = state
        .items
        .iter()
        .chain(&state.resolved_items)
        .chain(&synthetic)
        .find(|item| item.conversation_key() == key);
    let user = state.effective_user();
    let eligible = |item: &crate::domain::OpenItem| {
        item.status == ItemStatus::Open
            && item.authority != Authority::Agent
            && crate::core::routing::evaluate(item, &user, &state.config.stakeholders).is_eligible()
    };
    if let Some(item) = item {
        let context = format!(
            "{} · {}\n{}\n\nWhy this matters: {}\nRecommendation: {}\nRecorded evidence: {}\nStatus: {:?}",
            item.id,
            item.kind,
            item.question,
            item.reason,
            item.recommendation,
            item.evidence,
            item.status
        );
        let ask = if eligible(item) {
            if item.is_ownership_gap() {
                format!(
                    "Who should own {}? You can use this item's Assign ownership control.",
                    item.category
                )
            } else {
                item.question.clone()
            }
        } else {
            "How can I help you with this item?".into()
        };
        let greeting = format!(
            "We’re discussing {}: {}\n\n{}\n\n---\n- {ask}",
            item.id,
            item.question,
            clip(&item.reason, 1200)
        );
        return Some((context, greeting));
    }
    let doc = docs
        .iter()
        .find(|doc| doc.path == key && !doc.path.ends_with("/README.md"))?;
    // Only explicitly related open questions belong in a task's introduction.
    let pending = state
        .items
        .iter()
        .filter(|item| eligible(item))
        .filter(|item| {
            references::tokens(&doc.text).any(|token| token == item.id)
                || item.evidence.contains(&doc.path)
                || item.reason.contains(&doc.path)
        })
        .min_by_key(|item| (item.priority.rank(), &item.id));
    let ask = pending
        .map(|item| format!("{}: {}", item.id, item.question))
        .unwrap_or_else(|| format!("How can I help you with {}?", doc.title));
    let next = if pending.is_some() {
        format!("---\n- {ask}")
    } else {
        ask
    };
    Some((
        format!("{}\n{}", doc.path, doc.text),
        format!(
            "We’re discussing {}. The task description and acceptance criteria are available in Task context.\n\n{next}",
            doc.title
        ),
    ))
}

pub fn prompt(
    state: &PlannerState,
    key: &str,
    message: &str,
    history: &[(String, String)],
) -> Result<String, String> {
    let docs = crate::artifacts::task_docs::load_board(&state.planning_store, &state.workflow);
    let synthetic = crate::core::ownership::synthesize_for_state(state);
    let mut subject = if let Some(item) = state
        .items
        .iter()
        .chain(&state.resolved_items)
        .chain(&synthetic)
        .find(|item| item.conversation_key() == key)
    {
        serde_json::to_string_pretty(item).map_err(|e| e.to_string())?
    } else if let Some(doc) = docs
        .iter()
        .find(|doc| doc.path == key && !doc.path.ends_with("/README.md"))
    {
        format!("{}\n{}\n{}", doc.path, doc.title, doc.text)
    } else {
        let context = crate::core::planning_work::context(state, key)
            .ok_or_else(|| format!("Board item {key} no longer exists; refresh the board."))?;
        let work = crate::core::planning_work::find(state, key)
            .ok_or_else(|| format!("Board item {key} no longer exists; refresh the board."))?;
        let history = history
            .iter()
            .map(|(speaker, text)| format!("{speaker}: {}", clip(text, 2000)))
            .collect::<Vec<_>>()
            .join("\n");
        if work.kind == crate::core::planning_work::WorkKind::Question {
            return Ok(question::build(&context, &history, message));
        }
        if work.kind == crate::core::planning_work::WorkKind::DocumentationRefresh {
            return Ok(documentation_refresh::build(
                &context,
                &history,
                message,
                &state
                    .planning_store
                    .git_path(crate::artifacts::planning_store::paths::PRODUCT),
            ));
        }
        let context = format!(
            "{context}\nTask-specific guidance: {}",
            work.kind.planning_guidance()
        );
        return Ok(feature_planning::build(
            &context,
            &serde_json::to_string_pretty(&state.workflow.brief).unwrap_or_default(),
            &history,
            message,
            crate::artifacts::product_docs::next_feature_id(&state.planning_store),
        ));
    };
    if let Some(worker) =
        crate::core::implementation::load_with_store(&state.planning_store, &state.repo_root, key)
    {
        subject.push_str(&format!(
            "\nCurrent implementation status: {}\n{}\nPull request: {}\nMerged commit: {}",
            worker.status,
            clip(&worker.detail, 1500),
            worker.pr_url.as_deref().unwrap_or("none"),
            worker.merged_commit.as_deref().unwrap_or("none")
        ));
    }
    let related = state
        .items
        .iter()
        .chain(&state.resolved_items)
        .filter(|item| {
            item.conversation_key() != key
                && references::tokens(&subject).any(|token| token == item.id)
        })
        .collect::<Vec<_>>();
    let mut references = String::new();
    for id in references::tokens(&subject)
        .filter(|id| crate::artifacts::product_docs::valid_feature_id(id))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(3)
    {
        if let Ok(path) = crate::artifacts::product_docs::document_path(
            &state.planning_store,
            &format!("feature:{id}"),
        ) && let Ok(body) = std::fs::read_to_string(path)
        {
            references.push_str(&format!("\nFeature {id}\n{}", clip(&body, 10000)));
        }
    }
    if let Ok(Some(modules)) = crate::artifacts::product_docs::load_documents(&state.planning_store)
    {
        for document in modules {
            if subject.contains(&document.module.id) || subject.contains(&document.module.title) {
                references.push_str(&format!(
                    "\n{} ({})\n{}",
                    document.module.title,
                    document.module.path,
                    clip(&document.content, 8000)
                ));
            } else {
                let selected = references::referenced_sections(&document.content, &subject);
                if !selected.is_empty() {
                    references.push_str(&format!(
                        "\n{} ({})\n{selected}",
                        document.module.title, document.module.path
                    ));
                }
            }
        }
    } else if let Some(spec) = &state.spec_text {
        references.push_str(&references::referenced_sections(spec, &subject));
        if references.is_empty() {
            // Small project synopsis for an early, unreferenced question.
            references.push_str(&clip(spec.split("\n## ").next().unwrap_or(spec), 1200));
        }
    }
    for doc in &docs {
        let filename = std::path::Path::new(&doc.path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        if doc.path != key
            && !doc.path.ends_with("/README.md")
            && (subject.contains(&doc.path) || subject.contains(filename.as_ref()))
        {
            references.push_str(&format!(
                "\nExplicitly related task: {}\n{}",
                doc.path,
                clip(&doc.text, 4000)
            ));
        }
    }
    let history = history
        .iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|(speaker, text)| format!("{speaker}: {}", clip(text, 2000)))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "=== TASK CONVERSATION: {key} ===\nYou are continuing this board task, not generic Main Chat. Do not generate task stories or claim implementation work. Persist the user's answer through validated specification and item updates. Resolve only existing CLR-numbered items. Resolve Agent-owned uncertainty from evidence; add all actionable independent Human/Review items together, include their consequences and rationale, and set blocked_by for dependent decisions that must wait. Never ask an actionable question only in prose; summarize progress and point to the board. Ownership assignments are made through the item's Assign ownership control. Do not claim implementation worker status changes through prose.\n\nPROJECT: {}\nCURRENT USER: {:?}\n\nDURABLE TASK CONTEXT\n{}\n\nEXPLICITLY RELATED ITEMS\n{}\n\nREFERENCED SPECIFICATION\n{}\nRead referenced source artifacts if more context is needed; never write files directly.\n\nTHIS TASK'S CONVERSATION ONLY\n{history}\n\nUSER REPLY\n{message}",
        state.title,
        state.effective_user(),
        clip(&subject, 16000),
        serde_json::to_string(&related).unwrap_or_default(),
        clip(&references, 16000)
    ))
}

mod question;

#[cfg(test)]
#[path = "task_conversation/tests.rs"]
mod tests;
