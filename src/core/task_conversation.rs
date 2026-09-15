//! Focused prompts assembled from board identity and durable planning artifacts.
use crate::core::{context_build::clip, state::PlannerState};

fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|token| !token.is_empty())
}

/// Include sections defining explicitly referenced IDs, without copying the document.
fn referenced_sections(document: &str, subject: &str) -> String {
    let ids = tokens(subject)
        .filter(|token| {
            token.split_once('-').is_some_and(|(prefix, number)| {
                !prefix.is_empty()
                    && prefix.chars().all(|c| c.is_ascii_uppercase())
                    && !number.is_empty()
                    && number.chars().all(|c| c.is_ascii_digit())
            })
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut sections = Vec::new();
    let mut current = String::new();
    for line in document.lines() {
        if (line.starts_with('#') || line.starts_with("- **") || line.starts_with('|'))
            && !current.is_empty()
        {
            sections.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    sections.push(current);
    sections
        .into_iter()
        .filter(|section| {
            // A heading or table row defining the identifier is relevant; incidental
            // references elsewhere do not pull the whole project into this thread.
            section.lines().any(|line| {
                (line.starts_with('#') || line.starts_with('|') || line.starts_with("- **"))
                    && tokens(line).any(|token| ids.contains(token))
            })
        })
        .map(|section| clip(&section, 5000))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_match_whole_ids_in_headings_bullets_and_tables() {
        let document = "# Scope\nOverview\n- **FR-1** Use corporate SSO.\n  Require MFA.\n- **FR-10** Unrelated reporting.\n## D-2 Authentication decision\nUse the existing directory.\n## D-20 Reporting decision\nUnrelated exports.\n| A-3 | SSO is available |\n| A-30 | Reporting is enabled |";
        let selected = referenced_sections(document, "Implement FR-1 based on D-2 and A-3");
        assert!(selected.contains("Require MFA"));
        assert!(selected.contains("existing directory"));
        assert!(selected.contains("SSO is available"));
        assert!(!selected.contains("Unrelated"));
        assert!(!selected.contains("Reporting is enabled"));
    }

    #[test]
    fn task_story_prompt_includes_explicit_dependency_and_referenced_specification() {
        let root = std::env::temp_dir().join(format!("packet_task_prompt_{}", std::process::id()));
        let directory = "planning/tasks/authentication";
        std::fs::create_dir_all(root.join(directory)).unwrap();
        let mut state = PlannerState::load(&root).unwrap();
        state.spec_text = Some("# Product\n## FR-1 Sign in\nUse corporate SSO.\n## FR-10 Reporting\nUnrelated reporting details.".into());
        state
            .workflow
            .task_batches
            .push(crate::core::workflow::TaskBatchRef {
                feature: "Authentication".into(),
                directory: directory.into(),
                count: 3,
            });
        std::fs::write(
            root.join(directory).join("001-login.md"),
            "# Sign in\nImplement FR-1.\n## Dependencies\n[Directory](002-directory.md)",
        )
        .unwrap();
        std::fs::write(
            root.join(directory).join("002-directory.md"),
            "# Directory\nProvision the corporate tenant.",
        )
        .unwrap();
        std::fs::write(
            root.join(directory).join("003-reporting.md"),
            "# Reporting\nUNRELATED TASK DETAILS",
        )
        .unwrap();
        let body = prompt(
            &state,
            &format!("{directory}/001-login.md"),
            "Use the existing provider",
            &[("User".into(), "OUR TASK HISTORY".into())],
        )
        .unwrap();
        assert!(body.contains("Use corporate SSO."));
        assert!(body.contains("Provision the corporate tenant."));
        assert!(body.contains("OUR TASK HISTORY"));
        assert!(!body.contains("UNRELATED TASK DETAILS"));
        assert!(!body.contains("Unrelated reporting details"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

pub fn prompt(
    state: &PlannerState,
    key: &str,
    message: &str,
    history: &[(String, String)],
) -> Result<String, String> {
    let docs = crate::artifacts::task_docs::load_latest(&state.repo_root, &state.workflow);
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
        return Err(format!(
            "Board item {key} no longer exists; refresh the board."
        ));
    };
    if let Some(worker) = crate::core::implementation::load(&state.repo_root, key) {
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
            item.conversation_key() != key && tokens(&subject).any(|token| token == item.id)
        })
        .collect::<Vec<_>>();
    let mut references = String::new();
    for id in tokens(&subject)
        .filter(|id| crate::artifacts::product_docs::valid_feature_id(id))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(3)
    {
        if let Ok(path) = crate::artifacts::product_docs::document_path(
            &state.repo_root,
            &format!("feature:{id}"),
        ) {
            if let Ok(body) = std::fs::read_to_string(path) {
                references.push_str(&format!("\nFeature {id}\n{}", clip(&body, 10000)));
            }
        }
    }
    if let Ok(Some(modules)) = crate::artifacts::product_docs::load_modules(&state.repo_root) {
        for (name, body) in crate::artifacts::product_docs::MODULES
            .iter()
            .zip(modules.iter())
        {
            if subject.contains(name.trim_end_matches(".md")) {
                references.push_str(&format!("\n{name}\n{}", clip(body, 8000)));
            } else {
                let selected = referenced_sections(body, &subject);
                if !selected.is_empty() {
                    references.push_str(&format!("\n{name}\n{selected}"));
                }
            }
        }
    } else if let Some(spec) = &state.spec_text {
        references.push_str(&referenced_sections(spec, &subject));
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
        "=== TASK CONVERSATION: {key} ===\nYou are responding inside this item's single focused conversation. Main Chat remains the primary project planning interface. Do not start a new interview, generate tasks, advance the overall planning workflow, or ask about unrelated items. Persist significant answers, decisions and assumptions through validated document_updates and item changes, recording the actual conclusion in document_updates or item evidence. Resolve only existing CLR-numbered items. Ownership assignments are made through the item's Assign ownership control; explain that control when needed. Ask follow-up questions only about this item. Set next_question_id to null when no focused eligible question is needed. Do not claim implementation worker status changes through prose.\n\nPROJECT: {}\nCURRENT USER: {:?}\n\nDURABLE TASK CONTEXT\n{}\n\nEXPLICITLY RELATED ITEMS\n{}\n\nREFERENCED SPECIFICATION\n{}\nRead referenced source artifacts if more context is needed; never write files directly.\n\nTHIS TASK'S CONVERSATION ONLY\n{history}\n\nUSER REPLY\n{message}",
        state.title,
        state.effective_user(),
        clip(&subject, 16000),
        serde_json::to_string(&related).unwrap_or_default(),
        clip(&references, 16000)
    ))
}
