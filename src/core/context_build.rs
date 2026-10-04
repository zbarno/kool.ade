//! Assembly of the concrete planning-turn context: who's asking, the chat
//! so far, and every durable fact the agent may need (§18 context building).
//! Size discipline lives here — oversized feeds get clipped deterministically
//! before the prompt renderer sees them.

use crate::artifacts::config_io;
use crate::artifacts::items_io;
use crate::core::context_retrieval::{ContextSelection, RetrievedArea, RetrievedDocument};
use crate::core::repo_overview::{Overview, scan};
use crate::core::state::PlannerState;
use crate::domain::CurrentUser;

#[path = "context_build/mcp.rs"]
mod mcp;
#[path = "context_build/resolved_history.rs"]
mod resolved_history;

const CONVERSATION_TAIL_MESSAGES: usize = 6;
const MESSAGE_CLIP_CHARS: usize = 1200;

#[derive(Debug, Clone)]
pub struct ConversationLine {
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ImportRow {
    pub path: String,
    pub bytes: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct TurnContext {
    pub user: CurrentUser,
    /// Per-seat lane digest under the D-14 law (may be empty: guest seat
    /// or empty config inherit nothing). Rendered inside CURRENT USER.
    pub lane_note: String,
    pub repo_title: String,
    pub repository_map: String,
    pub next_feature_id: String,
    pub user_message: String,
    pub conversation: Vec<ConversationLine>,
    /// How many older messages were left out of `conversation`.
    pub elided_messages: usize,
    pub overview: Overview,
    pub spec_markdown: Option<String>,
    pub product_index: Option<String>,
    pub active_feature: Option<(String, String)>,
    pub selected_documents: Vec<RetrievedDocument>,
    pub selected_areas: Vec<RetrievedArea>,
    pub open_items_markdown: String,
    pub config_markdown: String,
    pub imports: Vec<ImportRow>,
    /// Server names only; commands, arguments, and credentials are withheld.
    pub mcp_summary: Option<String>,
}

impl TurnContext {
    pub fn build(state: &PlannerState, user_message: &str, recent: &[(String, String)]) -> Self {
        Self::build_with_retrieval(state, user_message, recent, None)
    }

    pub fn build_with_retrieval(
        state: &PlannerState,
        user_message: &str,
        recent: &[(String, String)],
        selection: Option<&ContextSelection>,
    ) -> Self {
        let recent_len = recent.len();
        let tail: Vec<&(String, String)> = recent
            .iter()
            .rev()
            .take(CONVERSATION_TAIL_MESSAGES)
            .collect();
        let mut convo = Vec::with_capacity(tail.len());
        for (speaker, text) in tail.iter().rev() {
            convo.push(ConversationLine {
                speaker: speaker.clone(),
                text: clip(text, MESSAGE_CLIP_CHARS),
            });
        }
        let overview = scan(&state.repo_root);
        let imports = derive_import_rows(state);
        let mcp_summary = mcp::summary(&state.repo_root);
        let layout = crate::artifacts::layout::ArtifactLayout::new(&state.repo_root);
        let product_index = std::fs::read_to_string(layout.product_index())
            .ok()
            .map(|s| clip(&s, 5000));
        let active_feature = product_index
            .as_ref()
            .and(state.active_feature.as_ref())
            .map(|(id, body)| (id.clone(), clip(body, 12000)));
        let selected_documents = selection
            .map(|selection| selection.documents.clone())
            .unwrap_or_default();
        let selected_areas = selection
            .map(|selection| selection.repository_areas.clone())
            .unwrap_or_default();
        let fallback_items = state.items.iter().take(12).cloned().collect::<Vec<_>>();
        let chosen_items = selection
            .map(|selection| &selection.open_items)
            .unwrap_or(&fallback_items);
        let open_items = clip(&items_io::serialize(chosen_items), 12000);
        TurnContext {
            user: state.effective_user(),
            lane_note: clip(
                &crate::core::routing::describe_lanes(
                    &state.effective_user(),
                    &state.config.stakeholders,
                ),
                4000,
            ),
            repo_title: state.title.clone(),
            next_feature_id: crate::artifacts::product_docs::next_feature_id(&state.repo_root),
            repository_map: clip(
                &state
                    .repositories
                    .repositories
                    .iter()
                    .map(|repo| format!("{}: {} ({})", repo.id, repo.role, repo.remote))
                    .collect::<Vec<_>>()
                    .join("\n"),
                5000,
            ),
            user_message: user_message.to_string(),
            conversation: convo,
            elided_messages: recent_len.saturating_sub(CONVERSATION_TAIL_MESSAGES),
            overview,
            spec_markdown: product_index
                .is_none()
                .then(|| state.spec_text.clone())
                .flatten(),
            product_index,
            active_feature,
            selected_documents,
            selected_areas,
            open_items_markdown: resolved_history::with_summary(open_items, &state.resolved_items),
            config_markdown: clip(&config_io::serialize(&state.config), 5000),
            imports,
            mcp_summary,
        }
    }
}

/// Clip free text to a character cap with a marker.
pub fn clip(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let mut out: String = s.chars().take(cap.saturating_sub(20)).collect();
    out.push_str("\n[…truncated…]");
    out
}

/// Imports are whatever sits under Koolade's canonical imports tree except the companion
/// sidecars the importer writes (.txt.md companions, .bin-note.md notes).
fn derive_import_rows(state: &PlannerState) -> Vec<ImportRow> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(&state.repo_root);
    let base = layout.imports_root();
    let mut rows = Vec::new();
    if base.is_dir()
        && let Ok(entries) = std::fs::read_dir(&base)
    {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".md") && {
                name.strip_suffix(".bin-note.md")
                    .map(|_| true)
                    .unwrap_or(false)
                    || name_has_source_twin(&e.path(), &base)
            } {
                continue;
            }
            let bytes = std::fs::metadata(e.path()).ok().map(|m| m.len() as usize);
            rows.push(ImportRow {
                path: e
                    .path()
                    .strip_prefix(&state.repo_root)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                bytes,
            });
        }
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows.truncate(30);
    rows
}

fn name_has_source_twin(candidate: &std::path::Path, base: &std::path::Path) -> bool {
    let Some(name) = candidate.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if let Some(stripped) = name.strip_suffix(".md") {
        if stripped.ends_with(".bin-note") {
            let source = stripped.strip_suffix(".bin-note").unwrap_or("");
            return base.join(source).is_file();
        }
        // foo.txt.md companion → foo.txt sibling
        return base.join(stripped).is_file();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> Vec<(String, String)> {
        (0..n)
            .map(|i| (format!("sp{i}"), format!("msg {i}")))
            .collect()
    }

    #[test]
    fn conversation_tail_bounded_and_ordered() {
        let convos: Vec<ConversationLine> = TailPreview::preview(&lines(10));
        assert_eq!(convos.len(), 6);
        assert_eq!(convos.first().unwrap().speaker, "sp4");
        assert_eq!(convos.last().unwrap().speaker, "sp9");
    }

    #[allow(dead_code)]
    struct TailPreview;
    impl TailPreview {
        fn preview(recent: &[(String, String)]) -> Vec<ConversationLine> {
            recent
                .iter()
                .rev()
                .take(CONVERSATION_TAIL_MESSAGES)
                .collect::<Vec<_>>()
                .iter()
                .rev()
                .map(|(s, t)| ConversationLine {
                    speaker: s.clone(),
                    text: clip(t, MESSAGE_CLIP_CHARS),
                })
                .collect()
        }
    }

    #[test]
    fn clips_long_messages() {
        let s = "z".repeat(5000);
        let c = clip(&s, 1200);
        assert!(c.chars().count() <= 1200);
        assert!(c.ends_with("[…truncated…]"));
    }

    #[test]
    fn historical_features_tasks_and_items_do_not_expand_normal_prompt_without_bound() {
        let root = std::env::temp_dir().join(format!(
            "koolade_context_growth_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
        crate::artifacts::product_docs::migrate(&root, &legacy).unwrap();
        let active = root.join(".koolade-packet/planning/changes/CHG-001-active/specification.md");
        std::fs::create_dir_all(active.parent().unwrap()).unwrap();
        std::fs::write(active,
            "# CHG-001: Active\n\n**Status:** Draft\n\n## Intent\n\nOne feature.\n\n## Current Behavior\n\nCurrent.\n\n## Desired Behavior\n\nDesired.\n\n## Scope\n\nOne area.\n\n## Affected Product Areas\n\n`current-capabilities.md`\n\n## Requirements\n\nOne.\n\n## Decisions and Assumptions\n\nNone.\n\n## Acceptance Criteria\n\nOne.\n").unwrap();
        let mut state = PlannerState::load(&root).unwrap();
        let before_ctx = TurnContext::build(&state, "Plan this feature", &lines(2));
        assert!(before_ctx.selected_documents.is_empty());
        let before = crate::core::prompt::render_prompt(&before_ctx)
            + &crate::core::prompt::workflow_context(
                &state,
                crate::core::workflow::TurnPurpose::Interview,
            );
        std::fs::create_dir_all(root.join(".koolade-packet/planning/imports")).unwrap();
        for n in 2..=250 {
            let name = format!("CHG-{n:03}-historical");
            let dir = root.join(".koolade-packet/planning/changes").join(name);
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(
                dir.join("specification.md"),
                format!("# Historical {n}\n\n**Status:** Implemented\n\nHISTORICAL_SECRET_{n}\n"),
            )
            .unwrap();
            std::fs::write(
                root.join(".koolade-packet/planning/imports")
                    .join(format!("source-{n:03}.txt")),
                "evidence",
            )
            .unwrap();
            state
                .workflow
                .task_batches
                .push(crate::core::workflow::TaskBatchRef {
                    identity: None,
                    feature: format!("Historical batch {n}"),
                    directory: format!(".koolade-packet/planning/tasks/historical-{n}"),
                    count: 1,
                });
            let mut item = crate::domain::OpenItem::new(
                format!("CLR-{n:03}"),
                crate::domain::Priority::Normal,
                crate::domain::ItemKind::Question,
                "General".into(),
                Some("All".into()),
                format!("Historical item {n}?"),
                "Long context ".repeat(100),
            );
            item.authority = crate::domain::Authority::Human;
            state.items.push(item);
        }
        let ctx = TurnContext::build(&state, "Plan this feature", &lines(1000));
        let after = crate::core::prompt::render_prompt(&ctx)
            + &crate::core::prompt::workflow_context(
                &state,
                crate::core::workflow::TurnPurpose::Interview,
            );
        assert!(
            after.len() < before.len() + 20_000,
            "prompt grew from {} to {}",
            before.len(),
            after.len()
        );
        assert!(!after.contains("HISTORICAL_SECRET_250"));
        assert!(ctx.conversation.len() <= 6);
        assert!(ctx.imports.len() <= 30);
        assert!(ctx.selected_documents.len() <= 5);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepted_feature_knowledge_survives_chat_expiration_and_derived_cache_deletion() {
        let root = std::env::temp_dir().join(format!(
            "koolade_context_authority_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(root.join(".koolade-packet/state/derived")).unwrap();
        let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
        crate::artifacts::product_docs::migrate(&root, &legacy).unwrap();
        let active = root.join(".koolade-packet/planning/changes/CHG-001-active/specification.md");
        std::fs::create_dir_all(active.parent().unwrap()).unwrap();
        std::fs::write(active,
            "# CHG-001: Active\n\n**Status:** Draft\n\n## Intent\n\nDURABLE_DECISION_MARKER\n\n## Current Behavior\n\nCurrent.\n\n## Desired Behavior\n\nDesired.\n\n## Scope\n\nOne area.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nOne.\n\n## Decisions and Assumptions\n\nAccepted.\n\n## Acceptance Criteria\n\nOne.\n").unwrap();
        std::fs::write(
            root.join(".koolade-packet/state/derived/summary.json"),
            "DISPOSABLE_CACHE_MARKER",
        )
        .unwrap();
        let conversations = (0..100)
            .map(|n| {
                (
                    "user".to_string(),
                    if n == 0 {
                        "EXPIRED_CHAT_MARKER".into()
                    } else {
                        format!("recent {n}")
                    },
                )
            })
            .collect::<Vec<_>>();
        let state = PlannerState::load(&root).unwrap();
        let first = crate::core::prompt::render_prompt(&TurnContext::build(
            &state,
            "Continue the active feature",
            &conversations,
        ));
        assert!(first.contains("DURABLE_DECISION_MARKER"));
        assert!(!first.contains("EXPIRED_CHAT_MARKER"));
        assert!(!first.contains("DISPOSABLE_CACHE_MARKER"));
        std::fs::remove_dir_all(root.join(".koolade-packet/state/derived")).unwrap();
        let reloaded = PlannerState::load(&root).unwrap();
        let rebuilt = crate::core::prompt::render_prompt(&TurnContext::build(
            &reloaded,
            "Continue the active feature",
            &[],
        ));
        assert!(rebuilt.contains("DURABLE_DECISION_MARKER"));
        assert!(!rebuilt.contains("DISPOSABLE_CACHE_MARKER"));
        let _ = std::fs::remove_dir_all(root);
    }
}
