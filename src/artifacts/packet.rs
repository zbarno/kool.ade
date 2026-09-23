//! Packet-owned working artifacts and ADR publication.
//!
//! `.kool-ade-packet` is intentionally separate from product documentation:
//! it contains resumable planning/task material and implementation evidence.
//! A verified implementation is summarized into a conventional top-level
//! `adr/` record in the implementation repository.

use std::path::{Path, PathBuf};

use crate::core::implementation::{Implementation, Report};

pub const PACKET_DIR: &str = ".kool-ade-packet";
pub const PACKET_PLANNING_DIR: &str = ".kool-ade-packet/planning";
pub const PACKET_IMPLEMENTATION_DIR: &str = ".kool-ade-packet/implementation";
pub const PACKET_TASKS_DIR: &str = ".kool-ade-packet/planning/tasks";
pub const ADR_DIR: &str = "adr";

/// Existing repositories retain their historical task tree until an
/// explicit migration is requested. Fresh repositories start in the Packet
/// workspace, so no new planning artifact is written to the legacy root.
pub fn task_dir(repo: &Path) -> String {
    if repo.join("planning/tasks").is_dir() {
        "planning/tasks".into()
    } else {
        PACKET_TASKS_DIR.into()
    }
}

pub fn packet_root(repo: &Path) -> PathBuf {
    repo.join(PACKET_DIR)
}

pub fn planning_root(repo: &Path) -> PathBuf {
    repo.join(PACKET_PLANNING_DIR)
}

pub fn implementation_root(repo: &Path) -> PathBuf {
    repo.join(PACKET_IMPLEMENTATION_DIR)
}

/// Convert the approved ticket and verified implementation evidence into an
/// ADR. Existing records are never overwritten; rerunning a conversion is
/// therefore safe and preserves immutable decision history.
pub(crate) fn publish_adr(
    repo: &Path,
    state: &Implementation,
    report: &Report,
    changed_paths: &str,
) -> anyhow::Result<PathBuf> {
    let title = state
        .ticket_text
        .lines()
        .next()
        .unwrap_or("Implement the approved change")
        .trim_start_matches('#')
        .trim();
    let slug = crate::artifacts::task_docs::slug(title);
    let dir = repo.join(ADR_DIR);
    std::fs::create_dir_all(&dir)?;
    let mut path = dir.join(format!("implement-{slug}.md"));
    let content = render_adr(title, state, report, changed_paths);
    if path.exists() {
        if std::fs::read_to_string(&path)? == content {
            return Ok(path);
        }
        let mut n = 2;
        loop {
            let candidate = dir.join(format!("implement-{slug}-{n}.md"));
            if !candidate.exists() {
                path = candidate;
                break;
            }
            n += 1;
        }
    }
    crate::artifacts::atomic_write(&path, &content)?;
    Ok(path)
}

fn render_adr(title: &str, state: &Implementation, report: &Report, changed: &str) -> String {
    let today = chrono::Utc::now().format("%Y-%m-%d");
    let verification = report
        .verification
        .iter()
        .map(|command| format!("- `{command}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let criteria = report
        .acceptance_criteria
        .iter()
        .map(|criterion| format!("- **{}** — {}", criterion.criterion, criterion.evidence))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "# {title}\n\n- Status: Accepted\n- Date: {today}\n- Ticket: `{}`\n- Implementation commit: `{}`\n\n## Context\n\nThis decision implements the approved Packet task and records the architectural intent that was carried into the verified change.\n\n{}\n\n## Decision\n\nImplement the approved design described by the ticket and its frozen specification. The verified implementation changed:\n\n{changed}\n\n## Consequences\n\n{}\n\n## Verification\n\n{verification}\n\n## Acceptance evidence\n\n{criteria}\n",
        state.ticket,
        state.verified_head.as_deref().unwrap_or("pending"),
        state.ticket_text.trim(),
        report.summary.trim(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::implementation::Criterion;

    #[test]
    fn publishes_an_adr_without_overwriting_an_existing_decision() {
        let root = std::env::temp_dir().join(format!(
            "packet-adr-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state = Implementation {
            ticket: ".kool-ade-packet/planning/tasks/001-add-cache.md".into(),
            ticket_text: "# Add cache\n\nUse a bounded cache.".into(),
            approved_specification: None,
            approved_product_context: None,
            completed_dependency_context: None,
            branch: "packet/add-cache".into(),
            base: "master".into(),
            base_commit: "abc".into(),
            worktree: root.clone(),
            status: "Verifying".into(),
            detail: String::new(),
            pr_url: None,
            verified_head: Some("def".into()),
            auto_merge: false,
            merged_commit: None,
            pr_state: None,
            pr_checked_at: None,
            pr_check_attempted_at: None,
            pr_check_error: None,
            cleanup: Default::default(),
        };
        let report = Report {
            status: "complete".into(),
            summary: "Bounded cache added.".into(),
            acceptance_criteria: vec![Criterion {
                criterion: "Cache is bounded".into(),
                evidence: "Unit test passes".into(),
            }],
            verification: vec!["cargo test --offline".into()],
            remaining: vec![],
        };
        let path = publish_adr(&root, &state, &report, "src/cache.rs").unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.contains("## Context"));
        assert!(first.contains("## Decision"));
        assert!(first.contains("## Consequences"));
        assert_eq!(
            publish_adr(&root, &state, &report, "src/cache.rs").unwrap(),
            path
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), first);
        let _ = std::fs::remove_dir_all(root);
    }
}
