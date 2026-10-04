//! Process-global accrual host: activation, span guards, ledger appends,
//! error drain. Metering is best-effort — resolution or write failures are
//! recorded for the activity surface and never disturb the work itself.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use super::{Accruer, AgentSpan, ClosedInterval, StartOutcome, StopReason, unix_now_seconds};
use crate::artifacts::time_ledger::append_row;

struct Host {
    session_id: String,
    accrued: Accruer,
    errors: Vec<String>,
}

static HOST: OnceLock<Mutex<Option<Host>>> = OnceLock::new();

fn locked_host<'a>() -> std::sync::MutexGuard<'a, Option<Host>> {
    HOST.get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    *locked_host() = None;
}

fn record_error(errors: &mut Vec<String>, note: String) {
    errors.push(note);
    if errors.len() > 16 {
        errors.remove(0);
    }
}

/// Ledger field hygiene (printable ASCII, no `:` or `,`): invalid values
/// fall back wholesale instead of truncating mid-value.
fn identifier(value: &str, fallback: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| (0x20..0x7f).contains(&b) && b != b':' && b != b',')
    {
        value.to_owned()
    } else {
        fallback.to_owned()
    }
}

/// Lexical-or-canonical equality: manifest targets are often recorded
/// relative to the planning root, while app paths arrive absolute.
fn same_place(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok().as_deref() == b.canonicalize().ok().as_deref()
}

/// Stable id of the manifest entry rooted at `root`; `"root"` when none
/// matches (the single-repository default).
pub fn root_repo_id(root: &Path, manifest: &crate::core::project_repos::ProjectManifest) -> String {
    for entry in &manifest.repositories {
        if let Ok(Some(target)) = manifest.target_if_available(root, &entry.id)
            && same_place(root, &target)
        {
            return entry.id.clone();
        }
    }
    "root".to_owned()
}

/// Activates metering for one connected project, replacing any previously
/// connected project's open state (slots are never carried across projects).
pub fn activate_project(
    root: &Path,
    manifest: &crate::core::project_repos::ProjectManifest,
    session_id: &str,
) -> String {
    let repo_id = root_repo_id(root, manifest);
    *locked_host() = Some(Host {
        session_id: identifier(session_id, "session"),
        accrued: Accruer::new(),
        errors: Vec::new(),
    });
    repo_id
}

/// Attribution for one run of `ticket`; `None` when the workspace id cannot
/// be resolved (metering skipped; the work is never disturbed).
pub fn span_for_ticket(
    planning_root: &Path,
    ticket_text: &str,
    metadata: Option<&crate::artifacts::task_docs::TaskMetadata>,
    task_uid: Option<&str>,
) -> Option<AgentSpan> {
    let manifest = crate::core::project_repos::ProjectManifest::load(planning_root).ok()?;
    let workspace_id =
        crate::core::implementation::task_repository_id(ticket_text, metadata, &manifest).ok()?;
    let ticket_stem = Path::new(ticket_text)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("task")
        .trim_end_matches(".md");
    let item_uid = identifier(
        task_uid
            .map(str::to_owned)
            .as_deref()
            .unwrap_or(ticket_stem),
        "task",
    );
    Some(AgentSpan {
        repo_root: planning_root.to_path_buf(),
        repo_id: root_repo_id(planning_root, &manifest),
        workspace_id: identifier(&workspace_id, "root"),
        item_uid,
    })
}

/// Keeps one agent span open until the process exits. Dropping without an
/// explicit close settles it as [`StopReason::Completed`] — RAII makes
/// every escape path (completion, error, budget expiry, cancellation) close
/// the interval.
pub struct SpanGuard {
    span: AgentSpan,
}

impl SpanGuard {
    /// Settles the span with `reason` at this instant.
    pub fn close(self, reason: StopReason) {
        end_span(&self.span, reason);
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        end_span(&self.span, StopReason::Completed);
    }
}

fn end_span(span: &AgentSpan, reason: StopReason) {
    let workspace = span.workspace_id.clone();
    let now = unix_now_seconds();
    let mut host = locked_host();
    let Some(active) = host.as_mut() else {
        return;
    };
    let settled = active.accrued.on_active_stop(&workspace, reason, now);
    if let Some(interval) = settled {
        persist(active, interval);
    }
}

/// Opens the span's workspace slot, returning `None` when metering is not
/// activated for a project or an overlapping slot already holds the
/// workspace (union-on-overlap).
pub fn span_begin(span: &AgentSpan) -> Option<SpanGuard> {
    let mut host = locked_host();
    let active = host.as_mut()?;
    let session = active.session_id.clone();
    let opened = active
        .accrued
        .on_active_start(span, &session, unix_now_seconds());
    (opened == StartOutcome::Opened).then(|| SpanGuard { span: span.clone() })
}

/// Application shutdown: settles every open slot as
/// `EndStatus::InterruptedDiscard` so restarts can never re-emit or
/// double-count them. Idempotent when no project is connected.
pub fn app_close_flush() {
    let mut host = locked_host();
    let Some(active) = host.as_mut() else {
        return;
    };
    for interval in active.accrued.on_app_close(unix_now_seconds()) {
        persist(active, interval);
    }
}

/// Drains buffered ledger write failures (bounded ring) for activity-surface
/// display; metering problems must surface, not vanish.
pub fn drain_errors() -> Vec<String> {
    locked_host()
        .as_mut()
        .map(|host| std::mem::take(&mut host.errors))
        .unwrap_or_default()
}

fn persist(host: &mut Host, interval: ClosedInterval) {
    let row = interval.to_row();
    let target = interval.span.repo_root.clone();
    if let Err(error) = append_row(&target, &row) {
        record_error(
            &mut host.errors,
            format!(
                "Time ledger: could not append the {} interval ({}s) to {}: {error}",
                interval.span.workspace_id,
                interval.duration_secs(),
                target.display()
            ),
        );
    }
}
