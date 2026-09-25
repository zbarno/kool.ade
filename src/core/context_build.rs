//! Assembly of the concrete planning-turn context: who's asking, the chat
//! so far, and every durable fact the agent may need (§18 context building).
//! Size discipline lives here — oversized feeds get clipped deterministically
//! before the prompt renderer sees them.

use crate::artifacts::config_io;
use crate::artifacts::items_io;
use crate::core::repo_overview::{Overview, scan};
use crate::core::state::PlannerState;
use crate::domain::CurrentUser;

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
    pub selected_modules: Vec<(String, String)>,
    pub open_items_markdown: String,
    pub config_markdown: String,
    pub imports: Vec<ImportRow>,
    /// Raw `.planner/mcp.json` when present (§6 — surfaced verbatim, §19).
    pub mcp_json: Option<String>,
}

impl TurnContext {
    pub fn build(state: &PlannerState, user_message: &str, recent: &[(String, String)]) -> Self {
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
        let mcp_json = std::fs::read_to_string(crate::artifacts::repo_artifact(
            &state.repo_root,
            crate::artifacts::MCP_CONFIG_FILE,
        ))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| clip(&s, 4096));
        let modular = crate::artifacts::product_docs::load_modules(&state.repo_root)
            .ok()
            .flatten();
        let mut product_index = None;
        let mut active_feature = None;
        let mut selected_modules = Vec::new();
        if let Some(modules) = modular {
            product_index = std::fs::read_to_string(
                state.repo_root.join(crate::artifacts::product_docs::INDEX),
            )
            .ok()
            .map(|s| clip(&s, 5000));
            let focus = state
                .active_feature
                .as_ref()
                .map(|(_, body)| body.as_str())
                .unwrap_or("");
            let mut numbers = Vec::new();
            for (n, name) in crate::artifacts::product_docs::MODULES.iter().enumerate() {
                let key = name.trim_end_matches(".md");
                let explicit =
                    user_message.contains(key) || user_message.contains(&format!("product:{key}"));
                let referenced = match n + 1 {
                    4 => user_message.contains("F-"),
                    5 => user_message.contains("FR-"),
                    6 => user_message.contains("NFR-"),
                    10 => user_message.contains("D-"),
                    _ => false,
                };
                if explicit || referenced {
                    numbers.push(n);
                }
            }
            if numbers.is_empty() {
                for (n, name) in crate::artifacts::product_docs::MODULES.iter().enumerate() {
                    if focus.contains(&format!("`{name}`")) {
                        numbers.push(n);
                    }
                    if numbers.len() >= 4 {
                        break;
                    }
                }
            }
            numbers.truncate(4);
            for n in numbers {
                selected_modules.push((
                    format!(
                        "product:{}",
                        crate::artifacts::product_docs::MODULES[n].trim_end_matches(".md")
                    ),
                    clip(&modules[n], 10000),
                ));
            }
            active_feature = state
                .active_feature
                .as_ref()
                .map(|(id, body)| (id.clone(), clip(body, 12000)));
        }
        let open_items = if product_index.is_some() {
            let mut relevant = state
                .items
                .iter()
                .filter(|item| user_message.contains(&item.id))
                .cloned()
                .collect::<Vec<_>>();
            for item in &state.items {
                if relevant.len() >= 12 {
                    break;
                }
                if !relevant.iter().any(|found| found.id == item.id) {
                    relevant.push(item.clone());
                }
            }
            clip(&items_io::serialize(&relevant), 12000)
        } else {
            items_io::serialize(&state.items)
        };
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
            selected_modules,
            open_items_markdown: format!(
                "{open_items}\n\nCompleted item outcomes (durable state):\n{}",
                clip(
                    &items_io::serialize(
                        &state
                            .resolved_items
                            .iter()
                            .rev()
                            .take(8)
                            .cloned()
                            .collect::<Vec<_>>()
                    ),
                    8000
                )
            ),
            config_markdown: clip(&config_io::serialize(&state.config), 5000),
            imports,
            mcp_json,
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

/// Imports are whatever sits under `planning/imports/` except the companion
/// sidecars the importer writes (.txt.md companions, .bin-note.md notes).
fn derive_import_rows(state: &PlannerState) -> Vec<ImportRow> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(&state.repo_root);
    let base = layout.legacy_imports_root();
    let mut rows = Vec::new();
    if base.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&base) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".md") && {
                    let stem_companion = name
                        .strip_suffix(".bin-note.md")
                        .map(|_| true)
                        .unwrap_or(false)
                        || name_has_source_twin(&e.path(), &base);
                    stem_companion
                } {
                    continue;
                }
                let bytes = std::fs::metadata(e.path()).ok().map(|m| m.len() as usize);
                rows.push(ImportRow {
                    path: layout.legacy_import_relative(&name).unwrap_or_default(),
                    bytes,
                });
            }
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
            "packet_context_growth_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(root.join("planning/features/CHG-001-active")).unwrap();
        std::fs::create_dir_all(root.join("planning/imports")).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
        std::fs::write(root.join("planning/specification.md"), &legacy).unwrap();
        crate::artifacts::product_docs::migrate(&root, &legacy).unwrap();
        std::fs::write(root.join("planning/features/CHG-001-active/specification.md"),
            "# CHG-001: Active\n\n**Status:** Draft\n\n## Intent\n\nOne feature.\n\n## Current Behavior\n\nCurrent.\n\n## Desired Behavior\n\nDesired.\n\n## Scope\n\nOne area.\n\n## Affected Product Areas\n\n`05-functional-requirements.md`\n\n## Requirements\n\nOne.\n\n## Decisions and Assumptions\n\nNone.\n\n## Acceptance Criteria\n\nOne.\n").unwrap();
        let mut state = PlannerState::load(&root).unwrap();
        let before_ctx = TurnContext::build(&state, "Plan this feature", &lines(2));
        assert_eq!(before_ctx.selected_modules.len(), 1);
        assert_eq!(
            before_ctx.selected_modules[0].0,
            "product:05-functional-requirements"
        );
        let before = crate::core::prompt::render_prompt(&before_ctx)
            + &crate::core::prompt::workflow_context(
                &state,
                crate::core::workflow::TurnPurpose::Interview,
            );
        for n in 2..=250 {
            let name = format!("CHG-{n:03}-historical");
            let dir = root.join("planning/features").join(name);
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(
                dir.join("specification.md"),
                format!("# Historical {n}\n\n**Status:** Implemented\n\nHISTORICAL_SECRET_{n}\n"),
            )
            .unwrap();
            std::fs::write(
                root.join("planning/imports")
                    .join(format!("source-{n:03}.txt")),
                "evidence",
            )
            .unwrap();
            state
                .workflow
                .task_batches
                .push(crate::core::workflow::TaskBatchRef {
                    feature: format!("Historical batch {n}"),
                    directory: format!("planning/tasks/historical-{n}"),
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
        assert!(ctx.selected_modules.len() <= 4);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepted_feature_knowledge_survives_chat_expiration_and_derived_cache_deletion() {
        let root = std::env::temp_dir().join(format!(
            "packet_context_authority_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(root.join("planning/features/CHG-001-active")).unwrap();
        std::fs::create_dir_all(root.join(".planner/derived")).unwrap();
        let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
        std::fs::write(root.join("planning/specification.md"), &legacy).unwrap();
        crate::artifacts::product_docs::migrate(&root, &legacy).unwrap();
        std::fs::write(root.join("planning/features/CHG-001-active/specification.md"),
            "# CHG-001: Active\n\n**Status:** Draft\n\n## Intent\n\nDURABLE_DECISION_MARKER\n\n## Current Behavior\n\nCurrent.\n\n## Desired Behavior\n\nDesired.\n\n## Scope\n\nOne area.\n\n## Affected Product Areas\n\n`product:05-functional-requirements`\n\n## Requirements\n\nOne.\n\n## Decisions and Assumptions\n\nAccepted.\n\n## Acceptance Criteria\n\nOne.\n").unwrap();
        std::fs::write(
            root.join(".planner/derived/summary.json"),
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
        std::fs::remove_dir_all(root.join(".planner/derived")).unwrap();
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
