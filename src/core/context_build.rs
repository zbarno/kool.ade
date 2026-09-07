//! Assembly of the concrete planning-turn context: who's asking, the chat
//! so far, and every durable fact the agent may need (§18 context building).
//! Size discipline lives here — oversized feeds get clipped deterministically
//! before the prompt renderer sees them.

use crate::core::repo_overview::{scan, Overview};
use crate::core::state::PlannerState;
use crate::domain::CurrentUser;
use crate::artifacts::config_io;
use crate::artifacts::items_io;

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
    pub repo_title: String,
    pub user_message: String,
    pub conversation: Vec<ConversationLine>,
    /// How many older messages were left out of `conversation`.
    pub elided_messages: usize,
    pub overview: Overview,
    pub spec_markdown: Option<String>,
    pub open_items_markdown: String,
    pub config_markdown: String,
    pub imports: Vec<ImportRow>,
    /// Raw `.planner/mcp.json` when present (§6 — surfaced verbatim, §19).
    pub mcp_json: Option<String>,
}

impl TurnContext {
    pub fn build(state: &PlannerState, user_message: &str, recent: &[(String, String)]) -> Self {
        let recent_len = recent.len();
        let tail: Vec<&(String, String)> = recent.iter().rev().take(CONVERSATION_TAIL_MESSAGES).collect();
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
        TurnContext {
            user: state.effective_user(),
            repo_title: state.title.clone(),
            user_message: user_message.to_string(),
            conversation: convo,
            elided_messages: recent_len.saturating_sub(CONVERSATION_TAIL_MESSAGES),
            overview,
            spec_markdown: state.spec_text.clone(),
            open_items_markdown: items_io::serialize(&state.items),
            config_markdown: config_io::serialize(&state.config),
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
    let base = crate::artifacts::repo_artifact(&state.repo_root, "planning/imports");
    let mut rows = Vec::new();
    if base.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&base) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".md") && {
                    let stem_companion = name.strip_suffix(".bin-note.md").map(|_| true).unwrap_or(false)
                        || name_has_source_twin(&e.path(), &base);
                    stem_companion
                } {
                    continue;
                }
                let bytes = std::fs::metadata(e.path()).ok().map(|m| m.len() as usize);
                rows.push(ImportRow {
                    path: format!("planning/imports/{name}"),
                    bytes,
                });
            }
        }
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
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
        (0..n).map(|i| (format!("sp{i}"), format!("msg {i}"))).collect()
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
}
