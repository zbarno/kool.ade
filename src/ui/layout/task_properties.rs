use super::board_state::task_board_column;
use crate::core::implementation::{ImplementationStatus, PullRequestState};
use crate::ui::Surface;
use egui::RichText;

pub(crate) fn paint_task_properties(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    doc: &crate::artifacts::task_docs::TaskDocument,
) {
    ui.add_space(10.0);
    ui.label(RichText::new(&doc.path).size(12.5).weak());
    let ticket = &doc.path;
    let state = s.implementation_state(ticket).cloned();
    if !ticket.ends_with("/README.md") {
        ui.horizontal_wrapped(|ui| {
            if let Some(record) = &state {
                let status = if matches!(
                    record.status,
                    ImplementationStatus::Preparing
                        | ImplementationStatus::Implementing
                        | ImplementationStatus::Verifying
                ) && !s.implementation_active(ticket)
                {
                    "Interrupted — ready to resume"
                } else {
                    record.status.label()
                };
                ui.label(RichText::new(status).size(12.0).weak());
                if let Some(url) = &record.pr_url {
                    ui.hyperlink_to("Open PR", url);
                }
            } else {
                ui.label(
                    RichText::new(
                        crate::core::implementation::BOARD_COLUMNS[task_board_column(s, ticket)],
                    )
                    .size(12.0)
                    .weak(),
                );
            }
        });
        if let Some(record) = &state {
            ui.label(
                RichText::new(record.worktree.display().to_string())
                    .size(12.5)
                    .weak(),
            );
            if let Some(checked) = &record.pr_checked_at {
                ui.label(
                    RichText::new(format!("PR last checked: {checked}"))
                        .size(12.5)
                        .weak(),
                );
            }
            if let Some(error) = &record.pr_check_error {
                ui.label(format!("PR check failed: {error}"));
            }
            if record.pr_state == Some(PullRequestState::Closed) {
                ui.label("PR closed without merging. Reopen the PR on GitHub to return this task to review.");
            }
            if record.status == ImplementationStatus::Blocked {
                ui.collapsing("Recovery details", |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            ui.label(&record.detail);
                        });
                });
            }
            if record.status == ImplementationStatus::Completed {
                if let Some(at) = &record.cleanup.completed_at {
                    ui.label(format!(
                        "Worktree cleanup completed: {at}. Verification evidence retained."
                    ));
                } else {
                    ui.label("Worktree cleanup pending; retried automatically while this project is open.");
                }
                if let Some(commit) = &record.merged_commit {
                    ui.label(format!(
                        "Merged into {} · {}",
                        record.base,
                        &commit[..commit.len().min(12)]
                    ));
                }
            }
        }
    }
    if let Some(record) = &state {
        ui.collapsing("Implementation properties", |ui| {
            for (label, value) in [
                ("Branch", record.branch.as_str()),
                ("Base", record.base.as_str()),
                ("Base commit", record.base_commit.as_str()),
                (
                    "Verified commit",
                    record.verified_head.as_deref().unwrap_or("Not verified"),
                ),
                (
                    "Merge commit",
                    record.merged_commit.as_deref().unwrap_or("Not merged"),
                ),
                (
                    "PR state",
                    record
                        .pr_state
                        .map(PullRequestState::label)
                        .unwrap_or("No PR"),
                ),
                (
                    "Last PR attempt",
                    record
                        .pr_check_attempted_at
                        .as_deref()
                        .unwrap_or("Not checked"),
                ),
                ("Publication state", record.status.publication().label()),
                (
                    "Publish mode",
                    if record.auto_merge {
                        "Automatic merge"
                    } else {
                        "Pull request"
                    },
                ),
            ] {
                ui.label(format!("{label}: {value}"));
            }
            ui.label(&record.detail);
        });
    }
}
