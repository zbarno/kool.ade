//! Assembly of the concrete planning-turn context: who's asking, the chat
//! so far, and every durable fact the agent may need (§18 context building).
//! Size discipline lives here — oversized feeds get clipped deterministically
//! before the prompt renderer sees them.

use crate::artifacts::config_io;
use crate::artifacts::items_io;
use crate::core::context_retrieval::{ContextSelection, RetrievedArea, RetrievedDocument};
use crate::core::repo_overview::{Overview, scan_for_store};
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
    pub content: Option<String>,
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
    pub product_directory: String,
    pub project_config_path: String,
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
        let overview = scan_for_store(&state.repo_root, &state.planning_store);
        let imports = derive_import_rows(state);
        let mcp_summary = mcp::summary(&state.repo_root);
        let product_index = state
            .planning_store
            .read(crate::artifacts::planning_store::paths::PRODUCT_INDEX)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|text| clip(&text, 5000));
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
            next_feature_id: crate::artifacts::product_docs::next_feature_id(&state.planning_store),
            product_directory: state
                .planning_store
                .git_path(crate::artifacts::planning_store::paths::PRODUCT),
            project_config_path: state
                .planning_store
                .git_path(crate::artifacts::planning_store::paths::PROJECT_CONFIG),
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
    const TOTAL_INLINE_CHARS: usize = 24_000;
    const PER_IMPORT_INLINE_CHARS: usize = 8_000;

    let imports = crate::artifacts::imports_io::list_imports_in_store(&state.planning_store)
        .unwrap_or_default();
    let names = imports
        .iter()
        .map(|entry| entry.name.clone())
        .collect::<std::collections::HashSet<_>>();
    let managed =
        state.planning_store.mode != crate::artifacts::planning_store::StoreMode::LegacyEmbedded;
    let mut inline_remaining = TOTAL_INLINE_CHARS;
    let mut rows = imports
        .into_iter()
        .filter(|entry| {
            !entry.name.ends_with(".bin-note.md")
                && !entry
                    .name
                    .strip_suffix(".md")
                    .is_some_and(|source| names.contains(source))
        })
        .map(|entry| {
            let content = if managed
                && inline_remaining > 0
                && crate::artifacts::imports_io::is_textual_import_name(&entry.name)
            {
                let relative = format!(
                    "{}/{}",
                    crate::artifacts::planning_store::paths::IMPORTS,
                    entry.name
                );
                state
                    .planning_store
                    .read(relative)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .filter(|text| !text.contains('\0'))
                    .map(|text| {
                        let content = clip(&text, inline_remaining.min(PER_IMPORT_INLINE_CHARS));
                        inline_remaining = inline_remaining.saturating_sub(content.chars().count());
                        content
                    })
            } else {
                None
            };
            ImportRow {
                path: format!(
                    "{}/{}",
                    state
                        .planning_store
                        .git_path(crate::artifacts::planning_store::paths::IMPORTS),
                    entry.name
                ),
                bytes: Some(entry.bytes as usize),
                content,
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows.truncate(30);
    rows
}

#[cfg(test)]
mod tests;
