//! Mutation phase (SPECIFICATION.md §16–§17): apply a validated turn to the
//! in-memory state, synthesize ownership items, atomically write changed
//! artifacts, and produce the checkpoint commit message. Git staging/commit
//! is sequenced by `core::turn` right after these writes succeed.

use crate::artifacts::{OPEN_ITEMS_FILE, SPEC_FILE, items_io, spec_doc};
use crate::core::ownership;
use crate::core::state::PlannerState;
use crate::core::validation::NormalizedTurn;

#[derive(Debug, Clone)]
pub struct ApplyReceipt {
    pub spec_written: bool,
    pub items_written: bool,
    /// Semantic checkpoint subject, planner-authored (§20).
    pub commit_message: String,
    /// Repo-relative paths to stage/commit (empty ⇒ no commit made).
    pub repo_relative_paths: Vec<String>,
    /// Newly synthesized ownership item ids (shown as a toast note).
    pub synthesized_open_items: Vec<String>,
}

pub fn apply(state: &mut PlannerState, nt: &NormalizedTurn) -> anyhow::Result<ApplyReceipt> {
    if let Some(batch) = &nt.task_batch {
        anyhow::ensure!(
            spec_doc::load(&state.repo_root)? == state.spec_text,
            "The specification changed during task generation; review it again before retrying"
        );
        anyhow::ensure!(
            crate::artifacts::product_docs::active_feature(&state.repo_root)
                == state.active_feature,
            "The active feature changed during task generation; review it again before retrying"
        );
        anyhow::ensure!(
            crate::artifacts::task_docs::load_workflow(&state.repo_root)? == state.workflow,
            "The interview changed during task generation; review it again"
        );
        let mut workflow = nt
            .workflow
            .clone()
            .unwrap_or_else(|| state.workflow.clone());
        let paths =
            crate::artifacts::task_docs::write_batch(&state.repo_root, batch, &mut workflow)?;
        state.workflow = workflow;
        return Ok(ApplyReceipt {
            spec_written: false,
            items_written: false,
            commit_message: format!(
                "planner: generate task stories for {}",
                batch.brief.feature_name
            ),
            repo_relative_paths: paths,
            synthesized_open_items: Vec::new(),
        });
    }
    let spec_pre_existed = state.baseline_spec.is_some();

    // 1) Resolutions remove items from the queue; the history stays in git.
    if !nt.resolved.is_empty() {
        for item in state
            .items
            .iter()
            .filter(|item| nt.resolved.contains(&item.id))
        {
            let mut resolved = item.clone();
            resolved.status = crate::domain::ItemStatus::Resolved;
            resolved
                .evidence
                .push_str(&format!("\n\nResolution outcome: {}", nt.assistant_message));
            state.resolved_items.retain(|old| old.id != resolved.id);
            state.resolved_items.push(resolved);
        }
        state
            .items
            .retain(|i| !nt.resolved.iter().any(|r| r == &i.id));
    }
    // 2) Field patches on surviving items.
    for (id, patch) in &nt.updates {
        if let Some(it) = state.items.iter_mut().find(|i| &i.id == id) {
            if let Some(p) = patch.priority {
                it.priority = p;
            }
            if let Some(authority) = patch.authority {
                it.authority = authority;
            }
            if let Some(k) = patch.kind {
                it.kind = k;
            }
            if let Some(c) = &patch.category {
                it.category = c.clone();
            }
            if let Some(a) = &patch.assigned_to {
                it.assigned_to = Some(a.clone());
            }
            if let Some(q) = &patch.question {
                it.question = q.clone();
            }
            if let Some(r) = &patch.reason {
                it.reason = r.clone();
            }
            if let Some(id) = &patch.feature_id {
                it.feature_id = Some(id.clone());
            }
            if let Some(value) = &patch.recommendation {
                it.recommendation = value.clone();
            }
            if let Some(value) = &patch.evidence {
                it.evidence = value.clone();
            }
        }
    }
    // 3) Agent-added items (validated + numbered already).
    state.items.extend(nt.added.iter().cloned());

    // 4) Ownership-gap synthesis (always after agent mutations, §8).
    let mut synthetic = ownership::synthesize_for_state(state);
    let synthetic_ids: Vec<String> = if synthetic.is_empty() {
        Vec::new()
    } else {
        let taken: Vec<String> = state
            .items
            .iter()
            .chain(&state.resolved_items)
            .map(|i| i.id.clone())
            .collect();
        ownership::assign_ids(&mut synthetic, taken);
        let ids = synthetic.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        state.items.extend(synthetic);
        ids
    };

    // 5) Canonical queue order + serialize.
    items_io::sort_queue(&mut state.items);
    let items_md = items_io::serialize(&state.items);

    // 6) Stage every changed artifact before writing any of them. The journal
    // restores the old complete set on an interrupted or failed apply.
    let mut changes = Vec::new();
    if !nt.resolved.is_empty() {
        changes.push((
            "planning/resolved-items.json".into(),
            serde_json::to_string_pretty(&state.resolved_items)?,
        ));
    }
    if let Some(spec) = &nt.spec_markdown {
        changes.push((SPEC_FILE.to_string(), spec.clone()));
    }
    for (id, content) in &nt.document_updates {
        let path = crate::artifacts::product_docs::document_path_for_update(
            &state.repo_root,
            id,
            content,
        )?;
        let content = if id == "product:index" {
            crate::artifacts::product_docs::refreshed_index_from(
                &state.repo_root,
                content,
                &nt.document_updates,
            )?
        } else {
            content.clone()
        };
        changes.push((
            path.strip_prefix(&state.repo_root)?
                .to_string_lossy()
                .into_owned(),
            content,
        ));
    }
    if !nt
        .document_updates
        .iter()
        .any(|(id, _)| id == "product:index")
    {
        if nt
            .document_updates
            .iter()
            .any(|(id, _)| id.starts_with("feature:"))
        {
            changes.push((
                crate::artifacts::product_docs::INDEX.into(),
                crate::artifacts::product_docs::refreshed_index(
                    &state.repo_root,
                    &nt.document_updates,
                )?,
            ));
        }
    }
    changes.push((OPEN_ITEMS_FILE.into(), items_md.clone()));
    if let Some(workflow) = &nt.workflow {
        changes.push((
            crate::core::workflow::WORKFLOW_FILE.into(),
            serde_json::to_string_pretty(workflow)?,
        ));
    }
    let repo_relative_paths = crate::artifacts::transaction::apply(&state.repo_root, &changes)?;
    let spec_written = repo_relative_paths.iter().any(|p| p == SPEC_FILE)
        || repo_relative_paths
            .iter()
            .any(|p| p.starts_with("planning/product/") || p.starts_with("planning/features/"));
    let items_written = repo_relative_paths.iter().any(|p| p == OPEN_ITEMS_FILE);
    state.spec_text = spec_doc::load(&state.repo_root)?;
    state.active_feature = crate::artifacts::product_docs::active_feature(&state.repo_root);
    state.baseline_spec = state.spec_text.clone();
    state.baseline_items_md = items_md;
    if let Some(workflow) = &nt.workflow {
        state.workflow = workflow.clone();
    }
    let commit_message = compose_commit_message(
        nt,
        spec_pre_existed,
        spec_written,
        nt.resolved.len(),
        nt.added.len(),
        nt.updates.len(),
    );
    Ok(ApplyReceipt {
        spec_written,
        items_written,
        commit_message,
        repo_relative_paths,
        synthesized_open_items: synthetic_ids,
    })
}

/// Subject like `planner: add caching policy` — agent-supplied summary when
/// sensible, otherwise derived from what actually happened (§20).
fn compose_commit_message(
    nt: &NormalizedTurn,
    spec_pre_existed: bool,
    spec_written: bool,
    n_resolved: usize,
    n_added: usize,
    n_updated: usize,
) -> String {
    let phrase_src = match (
        &nt.change_summary,
        spec_pre_existed,
        spec_written,
        n_resolved,
        n_added,
        n_updated,
    ) {
        (Some(summary), _, _, _, _, _) if !summary.is_empty() => summary.clone(),
        (_, false, true, _, _, _) => "establish initial specification".to_string(),
        (_, _, _, 0, 0, 0) => "refresh working notes".to_string(),
        (_, _, _, r, a, u) => {
            let mut parts = Vec::new();
            if r > 0 {
                parts.push(format!("resolve {r} open item{}", plural(r)));
            }
            if a > 0 {
                parts.push(format!("raise {a} open item{}", plural(a)));
            }
            if u > 0 {
                parts.push(format!("adjust {u} item{}", plural(u)));
            }
            if parts.is_empty() {
                "advance specification".to_string()
            } else {
                format!("advance specification ({})", parts.join(", "))
            }
        }
    };
    let mut phrase: String = phrase_src.chars().take(80).collect();
    if let Some((i, c)) = phrase.char_indices().next() {
        if c.is_ascii_uppercase() {
            phrase.replace_range(i..i + c.len_utf8(), &c.to_lowercase().to_string());
        }
    }
    if phrase.is_empty() {
        phrase = "advance specification".to_string();
    }
    format!("planner: {phrase}")
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::validation;
    use crate::domain::{CurrentUser, ItemKind, Priority};
    use crate::harness::TurnEnvelope;

    fn state_at(tag: &str) -> (PlannerState, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("packet_apply_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for args in [
            ["init"].as_slice(),
            ["config", "user.email", "packet@test.local"].as_slice(),
            ["config", "user.name", "Packet Test"].as_slice(),
        ] {
            let _ = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output();
        }
        let mut st = PlannerState::load(&root).unwrap();
        st.bootstrap_missing().unwrap();
        (st, root)
    }

    fn make_norm(change: Option<&str>, spec: Option<&str>) -> NormalizedTurn {
        NormalizedTurn {
            assistant_message: "done".into(),
            change_summary: change.map(str::to_string),
            spec_markdown: spec.map(str::to_string),
            document_updates: Vec::new(),
            added: vec![],
            updates: vec![],
            resolved: vec![],
            next_question_id: None,
            workflow: None,
            task_batch: None,
            warnings: vec![],
        }
    }

    #[test]
    fn writes_changed_files_and_labels_checkpoint() {
        let (mut st, root) = state_at("both");
        st.items.push(crate::domain::OpenItem::new(
            "CLR-001".into(),
            Priority::Blocking,
            ItemKind::Question,
            "General".into(),
            Some("All".into()),
            "deploy frequency?".into(),
            "ops cadence".into(),
        ));
        st.baseline_items_md = items_io::serialize(&[]); // queue as of the PREVIOUS commit
        let nt = make_norm(
            Some("Add caching policy"),
            Some("# Spec v2\nCache: Redis\n"),
        );
        let rc = apply(&mut st, &nt).unwrap();
        assert!(rc.spec_written && rc.items_written);
        assert_eq!(rc.repo_relative_paths, vec![SPEC_FILE, OPEN_ITEMS_FILE]);
        assert_eq!(rc.commit_message, "planner: add caching policy");
        let spec_disk =
            crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(&root, SPEC_FILE))
                .unwrap();
        assert!(spec_disk.contains("Redis"));
        let items_disk = crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(
            &root,
            OPEN_ITEMS_FILE,
        ))
        .unwrap();
        assert!(
            items_disk.contains("deploy frequency?"),
            "queued item must round-trip to disk"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_change_turn_touches_nothing() {
        let (mut st, root) = state_at("noop");
        let nt = make_norm(None, st.spec_text.clone().as_deref());
        let rc = apply(&mut st, &nt).unwrap();
        assert!(!rc.spec_written && !rc.items_written);
        assert!(rc.repo_relative_paths.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unowned_used_category_spawns_single_ownership_item() {
        let (mut st, root) = state_at("own");
        st.config.user = Some(CurrentUser::new("Zach", vec!["Engineering".into()]));
        st.items.push(crate::domain::OpenItem::new(
            "CLR-001".into(),
            Priority::Normal,
            ItemKind::Question,
            "Operations".into(),
            Some("Ops lead".into()),
            "who watches prod logs?".into(),
            String::new(),
        ));
        st.baseline_items_md = items_io::serialize(&st.items);
        let e = TurnEnvelope {
            schema_version: Some(1),
            assistant_message: Some("logged".into()),
            change_summary: None,
            document_updates: None,
            updated_specification: None,
            open_items_added: None,
            open_items_updated: None,
            open_items_resolved: None,
            next_question_id: None,
            interview: None,
            task_stories: None,
            task_outline: None,
        };
        let nt = validation::validate(&e, &st, &st.effective_user()).unwrap();
        let rc = apply(&mut st, &nt).unwrap();
        // Only the USED category (Operations) spawns an ownership item, and
        // exactly one; a re-apply must not multiply them.
        let n_own = st
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Ownership)
            .count();
        assert_eq!(n_own, 1);
        assert_eq!(rc.synthesized_open_items.len(), 1);
        let nt2 = validation::validate(&e, &st, &st.effective_user()).unwrap();
        let rc2 = apply(&mut st, &nt2).unwrap();
        assert!(rc2.synthesized_open_items.is_empty());
        assert_eq!(
            st.items
                .iter()
                .filter(|i| i.kind == ItemKind::Ownership)
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn commit_phrase_lowercases_agent_summary() {
        let (st, root) = state_at("msg");
        let nt = make_norm(Some("Establish initial specification"), None);
        assert_eq!(
            compose_commit_message(&nt, true, false, 0, 0, 0),
            "planner: establish initial specification"
        );
        let _ = (st, std::fs::remove_dir_all(&root));
    }
}
