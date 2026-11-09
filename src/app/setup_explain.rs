//! Deterministic plain-English explanations for machine setup operations.
//!
//! Consent screens, the "Prepare this computer" checklist, and task detail
//! panes share this content so people always see the same truthful account
//! of what changes, why, what it means for security, whether it can be
//! undone, and what happens if they decline.
//!
//! Content tiers:
//! - `Curated`: hand-written, static, for every known operation ID.
//! - `Parametric`: a vetted template filled only with tokens parsed from
//!   the task ID itself (provider, image profile) — nothing invented.
//! - `Fallback`: the task's own scanner-produced wording, lightly framed.
//!   Used only for IDs this version has never seen; it grants no autonomy.

mod table;
mod templates;

use crate::persistence::setup_tasks::SetupTaskV1;

pub enum SetupExplanation {
    Curated {
        entry: table::Entry,
        task_title: String,
    },
    Parametric(templates::Rendered),
    Fallback {
        title: String,
        explanation: String,
    },
}

impl SetupExplanation {
    pub fn explain(task: &SetupTaskV1) -> Self {
        if let Some(entry) = table::lookup(&task.id) {
            return Self::Curated {
                entry,
                task_title: clamp_chars(&task.title, 160),
            };
        }
        if let Some(rendered) = templates::render(task) {
            return Self::Parametric(rendered);
        }
        Self::Fallback {
            title: clamp_chars(&task.title, 160),
            explanation: clamp_chars(&task.explanation, 240),
        }
    }

    pub fn is_generated_fallback(&self) -> bool {
        matches!(self, Self::Fallback { .. })
    }

    pub fn headline(&self) -> String {
        match self {
            Self::Curated { task_title, .. } => {
                first_line_or(task_title, "Review this machine setup step")
            }
            Self::Parametric(rendered) => rendered.headline.clone(),
            Self::Fallback { title, .. } => {
                first_line_or(title, "Please review this machine check.")
            }
        }
    }

    pub fn what_changes(&self) -> Vec<String> {
        match self {
            Self::Curated { entry, .. } => entry.changes.iter().map(|s| s.to_string()).collect(),
            Self::Parametric(rendered) => rendered.what_changes.clone(),
            Self::Fallback { .. } => vec![
                "Until you explicitly approve a specific repair, nothing on this computer changes. Approval previews always show the exact operation before it runs.".into(),
            ],
        }
    }

    pub fn why(&self) -> String {
        match self {
            Self::Curated { entry, .. } => entry.why.to_owned(),
            Self::Parametric(rendered) => rendered.why.clone(),
            Self::Fallback {
                title, explanation, ..
            } => {
                if explanation.trim().is_empty() {
                    first_line_or(
                        title,
                        "This machine check could not be completed automatically in this version.",
                    )
                } else {
                    explanation.clone()
                }
            }
        }
    }

    pub fn security(&self) -> String {
        match self {
            Self::Curated { entry, .. } => entry.security.to_owned(),
            Self::Parametric(rendered) => rendered.security.clone(),
            Self::Fallback { .. } => {
                "Kool.ad/e never asks you for passwords, API keys, or security codes in chat. Any approval you grant scopes to one named operation only.".into()
            }
        }
    }

    pub fn reversibility(&self) -> String {
        match self {
            Self::Curated { entry, .. } => entry.reversibility.to_owned(),
            Self::Parametric(rendered) => rendered.reversibility.clone(),
            Self::Fallback { .. } => {
                "Depends on the step; the linked guidance explains how to undo it.".into()
            }
        }
    }

    pub fn rollback_note(&self) -> Option<String> {
        match self {
            Self::Curated { entry, .. } => entry.rollback.map(str::to_string),
            Self::Parametric(rendered) => rendered.rollback_note.clone(),
            Self::Fallback { .. } => None,
        }
    }

    pub fn if_declined(&self) -> String {
        match self {
            Self::Curated { entry, .. } => entry.declined.to_owned(),
            Self::Parametric(rendered) => rendered.if_declined.clone(),
            Self::Fallback { .. } => {
                "Nothing happens: the step stays open, the affected capability stays limited, and you can return to it anytime.".into()
            }
        }
    }
}

fn clamp_chars(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

fn first_line_or(text: &str, default: &str) -> String {
    text.lines()
        .find_map(|line| {
            let line = line.trim();
            (!line.is_empty()).then_some(line.to_string())
        })
        .unwrap_or_else(|| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::SetupExplanation;
    use crate::persistence::setup_tasks::State;

    fn task(id: &str) -> crate::persistence::setup_tasks::SetupTaskV1 {
        crate::persistence::setup_tasks::SetupTaskV1 {
            schema_version: 1,
            id: id.into(),
            title: format!("title of {id}"),
            explanation: String::new(),
            next_step: String::new(),
            dependencies: Vec::new(),
            blocked_by: Vec::new(),
            state: State::Detected,
            created_at_unix_ms: 1,
            observed_at_unix_ms: 1,
            scan_generation: 1,
            verified_at_unix_ms: None,
            verified_result: None,
            attempts: Vec::new(),
            events: Vec::new(),
            superseded_by: Vec::new(),
        }
    }

    #[test]
    fn curated_entries_win_over_fallback_with_complete_fields() {
        let ex = SetupExplanation::explain(&task("setup:docker:rootless"));
        assert!(!ex.is_generated_fallback());
        assert_eq!(ex.headline(), "title of setup:docker:rootless");
        assert!(!ex.what_changes().is_empty());
        assert!(!ex.why().is_empty());
        assert!(!ex.security().is_empty());
        assert!(!ex.reversibility().is_empty());
        assert!(!ex.if_declined().is_empty());
    }

    #[test]
    fn fallback_states_only_that_it_is_unlisted_and_asks_to_review() {
        let ex = SetupExplanation::explain(&task("setup:mystery:thing"));
        assert!(ex.is_generated_fallback());
        assert!(!ex.why().is_empty());
        assert!(!ex.what_changes().iter().any(|line| line.starts_with("-")));
    }

    #[test]
    fn every_curated_entry_resolves_through_the_public_surface() {
        for (id, _, why, security, rev, _, _) in super::table::ENTRIES {
            let ex = SetupExplanation::explain(&task(id));
            assert!(!ex.is_generated_fallback(), "{id} fell to fallback");
            assert_eq!(ex.why(), *why, "{id}");
            assert_eq!(ex.security(), *security, "{id}");
            assert_eq!(ex.reversibility(), *rev, "{id}");
        }
    }

    #[test]
    fn no_copy_solicits_secrets() {
        for (id, ..) in super::table::ENTRIES {
            let ex = SetupExplanation::explain(&task(id));
            let blob = format!(
                "{} {}\n{}\n{}\n{}",
                ex.headline(),
                ex.why(),
                ex.what_changes().join("\n"),
                ex.security(),
                ex.if_declined()
            )
            .to_lowercase();
            for forbidden in [
                "enter your password",
                "paste your api key",
                "your password is",
                "type the token",
            ] {
                assert!(!blob.contains(forbidden), "{id} :: {forbidden}");
            }
        }
    }
}
