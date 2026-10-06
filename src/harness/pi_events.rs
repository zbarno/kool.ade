//! Folding pi's NDJSON event stream (docs/json.md) into the pieces the
//! planner needs: the authoritative final assistant text, a live activity
//! preview, completion/error signals.
//!
//! Deliberately tolerant: unparsable lines are counted and forgotten — pi
//! may print startup chatter that is not JSON.

use super::LiveProgress;
mod fold;
mod helpers;
mod usage;
pub use fold::fold_line;
use std::{collections::BTreeMap, time::Instant};

/// Rolling state accumulated line by line.
#[derive(Debug, Default, Clone)]
pub struct EventFold {
    /// Final assistant text from the LAST `message_end` (authoritative).
    pub final_assistant_text: String,
    /// Most recent human-readable activity (e.g. tool being executed).
    pub last_activity: Option<String>,
    pub saw_agent_end: bool,
    pub error_hint: Option<String>,
    pub events_seen: usize,
    pub unparsed_lines: usize,
    pub tool_executions: usize,
    pub last_stop_reason: Option<String>,
    pub last_error_class: Option<&'static str>,
    blocks: BTreeMap<usize, (String, String)>,
    history: Vec<super::LivePost>,
    tool_posts: BTreeMap<String, (u64, usize)>,
    message_id: u64,
    prior_thoughts: String,
    prior_response: String,
    checklist: Vec<usize>,
    checklist_revision: u64,
    model_calls: Vec<super::ModelCallUsage>,
    call_started: Option<(String, chrono::DateTime<chrono::Utc>, Instant)>,
}

impl EventFold {
    pub fn preview(&self) -> LiveProgress {
        let current_text = strip_checklist_marker(&self.block_text("text"));
        let (response, specification) = super::live_preview::project(&current_text);
        LiveProgress {
            telemetry: Default::default(),
            posts: self
                .history
                .iter()
                .cloned()
                .chain(self.current_posts())
                .collect(),
            thoughts: joined(&self.prior_thoughts, &self.block_text("thinking")),
            response: joined(&self.prior_response, &response),
            specification,
            activity: self.last_activity.clone(),
            checklist: self.checklist.clone(),
            checklist_revision: self.checklist_revision,
            model_calls: self.model_calls.clone(),
        }
    }

    fn current_posts(&self) -> Vec<super::LivePost> {
        self.blocks
            .iter()
            .filter_map(|(index, (kind, text))| {
                let text = if kind == "text" {
                    super::live_preview::project(&strip_checklist_marker(text)).0
                } else {
                    text.clone()
                };
                if text.is_empty() {
                    return None;
                }
                Some(super::LivePost {
                    id: (self.message_id, *index),
                    kind: kind.clone(),
                    text,
                })
            })
            .collect()
    }

    fn block_text(&self, kind: &str) -> String {
        self.blocks
            .values()
            .filter(|(k, _)| k == kind)
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn joined(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_owned(),
        (_, true) => a.to_owned(),
        _ => format!("{a}\n\n{b}"),
    }
}

fn parse_checklist_marker(text: &str) -> Option<Vec<usize>> {
    let marker = "<!-- koolade-checklist:";
    let start = text.rfind(marker)? + marker.len();
    let end = text[start..].find("-->")? + start;
    let value = text[start..end].trim();
    let mut indexes = Vec::new();
    if !value.is_empty() {
        for index in value.split(',') {
            indexes.push(index.trim().parse().ok()?);
        }
    }
    indexes.sort_unstable();
    indexes.dedup();
    Some(indexes)
}

fn strip_checklist_marker(text: &str) -> String {
    let marker = "<!-- koolade-checklist:";
    let mut visible = text.to_owned();
    while let Some(start) = visible.rfind(marker) {
        let end = visible[start..]
            .find("-->")
            .map(|end| start + end + 3)
            .unwrap_or(visible.len());
        visible.replace_range(start..end, "");
    }
    visible.trim().to_owned()
}

#[cfg(test)]
mod tests;
